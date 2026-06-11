import { useMutation } from '@tanstack/react-query';
import { executeRcon } from '../lib/api';
import { Section } from '../components/Section';

export function Settings() {
  const { mutate: saveConfig, isPending } = useMutation({
    mutationFn: async (settings: Record<string, string>) => {
      const results = [];
      for (const [key, value] of Object.entries(settings)) {
        const result = await executeRcon(`set ${key} ${value}`);
        results.push(result);
      }
      return results;
    },
  });

  const handleSave = (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const formData = new FormData(e.currentTarget);
    const settings: Record<string, string> = {};
    
    const hostname = formData.get('hostname') as string;
    const skill = formData.get('skill') as string;
    const maxclients = formData.get('maxclients') as string;
    const timelimit = formData.get('timelimit') as string;
    const fraglimit = formData.get('fraglimit') as string;

    if (hostname) settings.hostname = hostname;
    if (skill) settings.skill = skill;
    if (maxclients) settings.maxclients = maxclients;
    if (timelimit) settings.timelimit = timelimit;
    if (fraglimit) settings.fraglimit = fraglimit;

    if (Object.keys(settings).length > 0) {
      saveConfig(settings);
    }
  };

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Settings</h1>

      <Section title="Server Settings">
        <form onSubmit={handleSave} className="space-y-4">
          <div>
            <label className="block text-sm font-medium mb-1">Hostname</label>
            <input
              type="text"
              name="hostname"
              defaultValue="HandsNet deathmatch"
              className="w-full p-2 bg-gray-700 border border-gray-600 rounded"
              maxLength={32}
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-1">Skill (0-4)</label>
            <input
              type="number"
              name="skill"
              defaultValue="3"
              min={0}
              max={4}
              className="w-full p-2 bg-gray-700 border border-gray-600 rounded"
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-1">Max Clients</label>
            <input
              type="number"
              name="maxclients"
              defaultValue="25"
              min={1}
              max={256}
              className="w-full p-2 bg-gray-700 border border-gray-600 rounded"
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-1">Time Limit (min)</label>
            <input
              type="number"
              name="timelimit"
              defaultValue="20"
              min={0}
              max={999}
              className="w-full p-2 bg-gray-700 border border-gray-600 rounded"
            />
          </div>

          <div>
            <label className="block text-sm font-medium mb-1">Frag Limit</label>
            <input
              type="number"
              name="fraglimit"
              defaultValue="25"
              min={0}
              max={999}
              className="w-full p-2 bg-gray-700 border border-gray-600 rounded"
            />
          </div>

          <button
            type="submit"
            disabled={isPending}
            className="w-full py-2 bg-blue-600 hover:bg-blue-700 rounded disabled:opacity-50"
          >
            {isPending ? 'Saving...' : 'Save Settings'}
          </button>
        </form>
      </Section>
    </div>
  );
}
