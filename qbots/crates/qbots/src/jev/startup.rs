//! # qbots::jev::startup — loud at startup (Plan 72 T7, *Failure policy*)
//!
//! Two fallbacks, deliberately different:
//! - a **configured-but-unreachable** endpoint (or the wrong model) **refuses to start** when
//!   `require` is true (the default once a `jev:` block exists) — a fleet silently running q3
//!   while you believe you are measuring a model is a voided experiment;
//! - an individual late/flat/malformed answer is silent q3, counted (the hub's job).
//!
//! `require: false` downgrades the refusal to one `error!`; jev bots then play as `jev_static`.

use std::sync::Arc;
use std::time::Duration;

use brain::jev::gate::GateThresholds;
use brain::jev::question::{QuestionKey, QuestionKind, QuestionSpec};

use super::hub::HubConfig;
use super::net::{post_json, Endpoint, PostError};
use super::transport::{DecisionTransport, HttpTransport};
use super::wire::validate;
use crate::config::JevCfg;

/// The command an operator should run when the endpoint is unhealthy.
pub const PROBE_HINT: &str = "cargo run -p qbots -- jev-probe health";

/// Startup checks get more time than a steady-state ask (a cold model may be slow to answer).
const HEALTH_DEADLINE: Duration = Duration::from_secs(2);

/// What the fleet does about jev.
pub enum JevStartup {
    /// No jev brain in this fleet, or no enabled `jev:` block: no probe, no hub, no socket.
    Disabled,
    /// `require: false` and the health check failed: jev bots play as `jev_static`.
    Degraded,
    /// Healthy: start the hub with this config and transport (T8).
    Ready {
        /// Hub tunables derived from the `jev:` block.
        hub: HubConfig,
        /// The production transport.
        transport: Arc<dyn DecisionTransport>,
    },
}

impl JevStartup {
    /// Start the hub for `n_bots` bot slots when `Ready`; `None` otherwise.
    pub fn start_hub(self, n_bots: usize) -> Option<Arc<super::hub::Hub>> {
        match self {
            JevStartup::Ready { hub, transport } => {
                Some(Arc::new(super::hub::Hub::start(hub, n_bots, transport)))
            }
            JevStartup::Disabled | JevStartup::Degraded => None,
        }
    }
}

/// Run the startup gate. `wanted`: does any bot in this fleet run the `jev` brain?
/// `Err` = refuse to start, with an actionable message.
pub async fn gate(cfg: &JevCfg, wanted: bool) -> Result<JevStartup, String> {
    if !wanted {
        return Ok(JevStartup::Disabled);
    }
    if !cfg.enabled {
        tracing::error!(
            "brain 'jev' selected but no enabled `jev:` block in the config — every jev bot \
             will play as jev_static (add `jev: {{ enabled: true }}` to measure the model)"
        );
        return Ok(JevStartup::Disabled);
    }
    let ep = Endpoint::parse(&cfg.endpoint)?;
    match health(&ep, &cfg.model, HEALTH_DEADLINE).await {
        Ok(()) => {
            tracing::info!(endpoint = %ep, model = %cfg.model, "jev endpoint healthy");
            Ok(JevStartup::Ready {
                hub: hub_config(cfg),
                transport: Arc::new(HttpTransport {
                    endpoint: ep,
                    deadline: Duration::from_millis(cfg.deadline_ms),
                }),
            })
        }
        Err(cause) if cfg.require => Err(format!(
            "brain 'jev' needs a reachable jev endpoint at {ep} ({cause}); try: {PROBE_HINT}"
        )),
        Err(cause) => {
            tracing::error!(
                endpoint = %ep,
                %cause,
                "jev endpoint unhealthy and `require: false` — jev bots will play as jev_static; \
                 try: {PROBE_HINT}"
            );
            Ok(JevStartup::Degraded)
        }
    }
}

/// One tiny `noul` request: reachable, speaking `/v1/systemone`, and serving `model`?
pub async fn health(ep: &Endpoint, model: &str, deadline: Duration) -> Result<(), String> {
    let asked = [QuestionSpec {
        key: QuestionKey::Posture,
        instructions: "Is this a health check?",
        kind: QuestionKind::Noul,
    }];
    let body = r#"{"state":"health check","questions":{"posture":{"type":"noul","instructions":"Is this a health check?"}}}"#;
    let bytes = post_json(ep, "/v1/systemone", body, deadline)
        .await
        .map_err(|e| match e {
            PostError::Transport(t) => format!("err={}", t.slug()),
            PostError::Malformed(k) => format!("err={}", k.slug()),
        })?;
    validate(&bytes, &asked, model, &GateThresholds::default())
        .map(|_| ())
        .map_err(|k| format!("err={}", k.slug()))
}

/// Hub tunables from the `jev:` block.
pub fn hub_config(cfg: &JevCfg) -> HubConfig {
    HubConfig {
        model: cfg.model.clone(),
        max_concurrent: cfg.max_concurrent.max(1),
        max_input_tokens_per_s: cfg.max_input_tokens_per_s,
        max_state_tokens: cfg.max_state_tokens,
        snapshot_ttl: Duration::from_millis(cfg.snapshot_ttl_ms),
        ttl: Duration::from_secs_f64(cfg.ttl_s),
        min_dwell: Duration::from_secs_f64(cfg.min_dwell_s),
        event_floor: Duration::from_secs_f64(cfg.event_floor_s),
        jitter_max: cfg.dwell_jitter_max,
        gate: GateThresholds {
            margin_binary: cfg.gate_margin_binary,
            margin_multi: cfg.gate_margin_multi,
            ..GateThresholds::default()
        },
        journal: cfg.journal.then(|| {
            let ts = time::OffsetDateTime::now_utc().unix_timestamp();
            std::path::PathBuf::from(format!("logs/jev/journal-{ts}.jsonl"))
        }),
        ..HubConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled(endpoint: &str, require: bool) -> JevCfg {
        JevCfg {
            enabled: true,
            endpoint: endpoint.into(),
            require,
            ..JevCfg::default()
        }
    }

    #[tokio::test]
    async fn jev_startup_absent_block_or_no_jev_bot_is_disabled_without_a_probe() {
        // Both return before any socket: the endpoint below would refuse.
        let absent = JevCfg::default();
        assert!(!absent.enabled);
        assert!(matches!(
            gate(&absent, true).await,
            Ok(JevStartup::Disabled)
        ));
        assert!(matches!(
            gate(&enabled("127.0.0.1:1", true), false).await,
            Ok(JevStartup::Disabled)
        ));
    }

    #[tokio::test]
    async fn jev_startup_require_true_refuses_with_the_probe_hint() {
        // 127.0.0.1:1 — nothing listens; the box is not needed for this test.
        let err = match gate(&enabled("127.0.0.1:1", true), true).await {
            Err(e) => e,
            Ok(_) => panic!("must refuse to start"),
        };
        assert!(err.contains(PROBE_HINT), "{err}");
        assert!(err.contains("err=refused"), "{err}");
    }

    #[tokio::test]
    async fn jev_startup_require_false_degrades_to_jev_static() {
        assert!(matches!(
            gate(&enabled("127.0.0.1:1", false), true).await,
            Ok(JevStartup::Degraded)
        ));
    }

    #[test]
    fn jev_startup_yaml_block_defaults_require_true_and_maps_to_the_hub() {
        let yaml = "enabled: true\nendpoint: 127.0.0.1:8710\ngate_margin_binary: 0.6\n";
        let cfg: JevCfg = serde_yaml::from_str(yaml).expect("yaml");
        assert!(
            cfg.require,
            "require defaults to true once the block exists"
        );
        let hub = hub_config(&cfg);
        assert_eq!(hub.max_concurrent, 3);
        assert_eq!(
            hub.gate.margin_binary, 0.6,
            "the YAML value wins over the frozen 0.30 default"
        );
        assert_eq!(hub.event_floor, Duration::from_secs(5));
        assert_eq!(hub.ttl, Duration::from_secs(24));
    }
}
