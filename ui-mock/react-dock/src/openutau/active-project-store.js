const ACTIVE_PROJECT_KEY = 'openutau.activeProject.v1';

function browserStorage() {
  try { return globalThis.localStorage ?? null; } catch { return null; }
}

export function readActiveProjectName(storage = browserStorage()) {
  try {
    const name = storage?.getItem(ACTIVE_PROJECT_KEY);
    return typeof name === 'string' && name.trim() ? name : null;
  } catch {
    return null;
  }
}

export function setActiveProjectName(name, storage = browserStorage()) {
  try {
    if (typeof name === 'string' && name.trim()) storage?.setItem(ACTIVE_PROJECT_KEY, name);
    else storage?.removeItem(ACTIVE_PROJECT_KEY);
  } catch {
    // Project persistence must continue to work when browser storage is unavailable.
  }
}

export async function restoreActiveProject(engine, storage = browserStorage()) {
  const name = readActiveProjectName(storage);
  if (!name || typeof engine?.listProjects !== 'function' || typeof engine?.openProject !== 'function') return false;

  let projects;
  try {
    projects = await engine.listProjects();
  } catch {
    return false;
  }

  const exists = Array.isArray(projects) && projects.some(entry => {
    const storedName = typeof entry === 'string' ? entry : entry?.name;
    return storedName === name;
  });
  if (!exists) {
    setActiveProjectName(null, storage);
    return false;
  }

  try {
    await engine.openProject(name);
    return true;
  } catch {
    // Keep the remembered name so a transient store failure can be retried later.
    return false;
  }
}
