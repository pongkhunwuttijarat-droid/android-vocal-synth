// Dock Manager — show, hide and re-open the clone's dock panels.
//
// The clone page ships a fixed four-panel layout (`layoutPanels` in main.jsx),
// so a panel closed with its tab's ✕ is otherwise gone for the rest of the
// session: there is no menu and no way to add it back. This is the same manager
// the main editor already has (`DockManager` in ../main.jsx), rebuilt on the
// clone's role tokens so it follows the theme like everything else.
//
// Re-opening needs the same `position` the initial layout used, otherwise the
// panel lands in an arbitrary group. `REOPEN_POSITION` mirrors `layoutPanels`;
// if the reference panel is also closed, `below` on the arrangement still gives
// a sensible home instead of throwing.
import React, { useEffect, useRef, useState } from 'react';

const PANELS = [
  { id: 'arrangement', name: 'Arrangement', hint: 'Tracks, singers, parts and their volumes' },
  { id: 'roll', name: 'Piano Roll', hint: 'Notes on the keyboard grid for the active part' },
  { id: 'lanes', name: 'Expression Lanes', hint: 'One §6.6.1 expression curve at a time' },
  { id: 'properties', name: 'Note Properties', hint: 'Lyric, tone and expressions of the selection' },
];

/** Same anchors as the initial layout, so a re-opened panel lands where it began. */
const REOPEN_POSITION = {
  arrangement: { referencePanel: 'roll', direction: 'left' },
  roll: { referencePanel: 'arrangement', direction: 'below' },
  lanes: { referencePanel: 'roll', direction: 'below' },
  properties: { referencePanel: 'roll', direction: 'right' },
};

export function DockManager({ api, onClose }) {
  const closeRef = useRef(null);
  // The key handler must see the latest `onClose` without re-subscribing.
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  // `api.panels` is a live array, not React state: without this the dialog keeps
  // rendering the layout from the moment it opened, so a toggle looks like it did
  // nothing even though Dockview acted on it.
  const [, setRevision] = useState(0);
  useEffect(() => {
    if (!api) return undefined;
    const bump = () => setRevision(value => value + 1);
    const subscriptions = [api.onDidAddPanel(bump), api.onDidRemovePanel(bump)];
    return () => subscriptions.forEach(subscription => subscription.dispose());
  }, [api]);

  useEffect(() => {
    const previous = document.activeElement;
    closeRef.current?.focus();
    const onKeyDown = event => { if (event.key === 'Escape') onCloseRef.current(); };
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      // Focus goes back where it was, so a keyboard user is not dumped on <body>.
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);

  if (!api) return null;
  const byId = new Map(api.panels.map(panel => [panel.id, panel]));
  const add = id => {
    const anchor = REOPEN_POSITION[id];
    // A closed reference panel leaves `direction` dangling; fall back to the
    // arrangement, which is the root of the default layout.
    const position = byId.has(anchor.referencePanel)
      ? anchor
      : { referencePanel: 'arrangement', direction: anchor.direction };
    api.addPanel({ id, component: id, title: PANELS.find(panel => panel.id === id).name, position });
  };

  return <div className="ou-manager-backdrop" role="presentation" onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}>
    <section className="ou-manager" role="dialog" aria-modal="true" aria-labelledby="ou-manager-title" aria-describedby="ou-manager-note">
      <header>
        <div>
          <h2 id="ou-manager-title">Dock Manager</h2>
          <p>Panels can be shown, hidden and re-opened. Drag tabs to rearrange, drag splitters to resize, or float a panel out.</p>
        </div>
        <button ref={closeRef} type="button" className="ou-manager-close" aria-label="Close Dock Manager" onClick={onClose}>Close</button>
      </header>
      <div className="ou-manager-list">
        {PANELS.map(entry => {
          const panel = byId.get(entry.id);
          const open = Boolean(panel);
          // A floated panel has no group, so report that instead of a group id.
          const where = !open ? 'Hidden' : panel.api.group?.id ? `Group ${panel.api.group.id}` : 'Floating';
          return <div className="ou-manager-row" key={entry.id}>
            <span className="ou-manager-names">
              <strong>{entry.name}</strong>
              <small>{entry.hint}</small>
            </span>
            <span className={`ou-manager-where ${open ? '' : 'muted'}`}>{where}</span>
            <button type="button" role="switch" aria-checked={open} aria-label={`${entry.name} visible`}
              className={`ou-manager-switch ${open ? 'on' : ''}`}
              onClick={() => (open ? panel.api.close() : add(entry.id))}>
              <span />
            </button>
          </div>;
        })}
      </div>
      <p id="ou-manager-note" className="ou-manager-foot">Toggling never touches the project: hiding a panel is view state, and the Rust document is untouched. Press Escape to close.</p>
    </section>
  </div>;
}