import { PlayerList } from '../components/PlayerList';
import { Section } from '../components/Section';

export function Players() {
  return (
    <div className="space-y-6">
      <Section title="Connected Players">
        <PlayerList />
      </Section>
    </div>
  );
}
