import { num } from '../lib/format';
import type { Ingest } from '../lib/api';

/**
 * Shown only when something is wrong.
 *
 * `logs_readable === false` is the uid-mismatch signature: nginx and the stats
 * service running as different uids, whose only other symptom is a dashboard of
 * zeros. Putting a sentence on screen instead of leaving someone to debug that
 * is the entire point of instrumenting it.
 */
export function IngestHealth({ ingest }: { ingest: Ingest }) {
  const problems: { title: string; detail: string }[] = [];

  if (!ingest.logs_readable) {
    problems.push({
      title: 'Cannot read the nginx log directory',
      detail:
        'The proxy and this service must run as the SAME uid:gid and be launched the same way (same --userns flags). Until that is fixed every number here will read zero.',
    });
  }
  if (ingest.lag_seconds > 60) {
    problems.push({
      title: `Ingest is ${num(ingest.lag_seconds)}s stale`,
      detail: 'The reader has not completed a tick recently. These numbers are frozen.',
    });
  }
  if (ingest.parse_errors > 0) {
    problems.push({
      title: `${num(ingest.parse_errors)} unparseable log lines`,
      detail:
        'Those requests are missing from every figure below. A dashboard that is confidently wrong is worse than one that says so.',
    });
  }

  if (problems.length === 0) return null;

  return (
    <div className="space-y-2">
      {problems.map((p) => (
        <div
          key={p.title}
          className="rounded-lg border border-rose-500/40 bg-rose-500/10 p-3 text-sm"
          role="alert"
        >
          <div className="font-semibold text-rose-300">{p.title}</div>
          <div className="mt-0.5 text-rose-200/80">{p.detail}</div>
        </div>
      ))}
    </div>
  );
}
