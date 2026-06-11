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
  const currentDmflags = 17424;

  return (
    <div className="space-y-6">
      <Section title="Deathmatch Flags">
        <DmflagsPreset currentValue={currentDmflags} />
      </Section>
      <Section title="Flag Bits">
        <DmflagsBits currentValue={currentDmflags} />
      </Section>
      <Section title="Time Limit">
        <TimelimitControl />
      </Section>
      <Section title="Frag Limit">
        <FraglimitControl />
      </Section>
      <Section title="Map">
        <RestartMap currentMap="q2dm1" />
      </Section>
    </div>
  );
}
