import { useQuery } from '@tanstack/react-query';
import { getStatus } from '../lib/api';
import { useChanges } from '../contexts/ChangesContext';
import { DmflagsPreset } from '../components/DmflagsPreset';
import { DmflagsBits } from '../components/DmflagsBits';
import { TimelimitControl } from '../components/TimelimitControl';
import { FraglimitControl } from '../components/FraglimitControl';
import { RestartMap } from '../components/RestartMap';

interface SectionProps {
  title: string;
  children: React.ReactNode;
}

function Section({ title, children }: SectionProps) {
  return (
    <section className="p-4 bg-gray-800 rounded-lg">
      <h2 className="text-lg font-semibold mb-4">{title}</h2>
      {children}
    </section>
  );
}

export function Deathmatch() {
  const { getServerValue } = useChanges();
  
  // Use values from ChangesContext (which is updated by ServerStatusSync)
  const currentDmflags = Number(getServerValue('dmflags'));
  const currentTimelimit = Number(getServerValue('timelimit'));
  const currentFraglimit = Number(getServerValue('fraglimit'));
  
  // Get map from React Query for display
  const { data: status } = useQuery({
    queryKey: ['status'],
    queryFn: getStatus,
    refetchInterval: 2000,
  });

  return (
    <div className="space-y-6">
      <Section title="Deathmatch Flags">
        <DmflagsPreset currentValue={currentDmflags} />
      </Section>
      <Section title="Flag Bits">
        <DmflagsBits currentValue={currentDmflags} />
      </Section>
      <Section title="Time Limit">
        <TimelimitControl currentValue={currentTimelimit} />
      </Section>
      <Section title="Frag Limit">
        <FraglimitControl currentValue={currentFraglimit} />
      </Section>
      <Section title="Map">
        <RestartMap currentMap={status?.map ?? undefined} />
      </Section>
    </div>
  );
}
