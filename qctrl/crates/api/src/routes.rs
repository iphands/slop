use qctrl_rcon::RconClient;

use crate::config::Config;
use crate::maps::MapCache;

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub rcon_client: std::sync::Arc<RconClient>,
    pub map_cache: MapCache,
}
