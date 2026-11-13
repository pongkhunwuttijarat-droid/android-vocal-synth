export async function loadSingerChoices({ backendKind, baseUrl = '', getInstalledRoots, fetchImpl = globalThis.fetch }) {
  if (backendKind !== 'http') {
    const roots = await getInstalledRoots?.();
    return (Array.isArray(roots) ? roots : [])
      .filter(root => typeof root === 'string' && root.trim())
      .map(root => ({ id: root, label: root }));
  }

  if (typeof fetchImpl !== 'function') throw new Error('fetch is not available');
  const origin = String(baseUrl || '').replace(/\/+$/, '');
  if (!origin) throw new Error('HTTP singer choices need a server URL');
  const response = await fetchImpl(`${origin}/voicebanks`);
  if (!response.ok) throw new Error(`Could not list server voicebanks (${response.status})`);

  const payload = await response.json();
  const voicebanks = Array.isArray(payload?.voicebanks) ? payload.voicebanks : [];
  return voicebanks
    .filter(bank => typeof bank?.dir === 'string' && bank.dir.trim() && Number(bank.wav_count) > 0)
    .map(bank => ({
      id: bank.dir,
      label: typeof bank.name === 'string' && bank.name.trim() ? bank.name : bank.dir,
    }));
}
