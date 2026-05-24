// React render of MCK-VIEWER-FULL — paired with
// .mdd/models/objective/mockups/viewer-full.puml.
// Every UIC- element in that Salt contract appears here as data-testid="UIC-...";
// the Playwright parity spec .mdd/tests/ui/viewer-full.spec.ts asserts each one.
export function ViewerFull() {
  return (
    <div className="vf-window">
      <div className="vf-topbar">
        <span className="vf-brand">mdd</span>
        <span className="vf-path">/path/to/project</span>
      </div>

      <div className="vf-body">
        {/* LEFT — MODELS rail */}
        <aside className="vf-rail">
          <div className="vf-rail-head">
            <span>MODELS</span>
            <button data-testid="UIC-MODELS-COLLAPSE" title="Collapse or expand the MODELS panel">‹</button>
          </div>
          <div className="vf-toggle">
            <button data-testid="UIC-RAIL-MODE-DIRECTORY" className="active">Directory</button>
            <button data-testid="UIC-RAIL-MODE-BYCYCLE">By cycle</button>
          </div>
          <ul className="vf-tree" data-testid="UIC-MODEL-TREE" role="tree" aria-label="Model file tree">
            <li>.mdd/models</li>
            <li className="indent">current</li>
            <li className="indent2">domain</li>
            <li className="indent3 sel">canvas-view.puml</li>
            <li className="indent2">mockups</li>
            <li className="indent3">viewer-full.puml</li>
            <li className="indent">objective</li>
          </ul>
          <button className="vf-deploy" data-testid="UIC-DEPLOY-SECTION">▸ DEPLOY · /mdd-deploy utility, not parity-gated</button>
        </aside>

        {/* CENTER — view tabs + canvas + diff/ocl bars */}
        <main className="vf-center">
          <div className="vf-tabs">
            <button data-testid="UIC-VIEW-TAB-DIAGRAM" className="active">Diagram</button>
            <button data-testid="UIC-VIEW-TAB-SOURCE">Source</button>
            <button data-testid="UIC-VIEW-TAB-DIFF">Diff</button>
            <span className="vf-spacer" />
            <button data-testid="UIC-PAGE-SELECTOR" className="vf-page" title="Diagram page selector">PAGE 1 2 3</button>
            <button data-testid="UIC-OPEN-RENDER" className="vf-page" title="Open the React render of the selected mockup in the browser (contextual: enabled when a mockup with a render is selected)">Open render ↗</button>
          </div>

          <div
            className="vf-canvas"
            data-testid="UIC-DIAGRAM-CANVAS"
            role="img"
            aria-label="Diagram canvas with center fisheye, scroll zoom and drag pan"
          >
            <span>canvas — centre bulges (fisheye), corners stay fixed</span>
          </div>

          <div className="vf-diffbar">
            <span>DIFF&nbsp;·&nbsp;</span>
            <button data-testid="UIC-DIFF-CYCLE-PICKER" className="vf-page" title="Diff cycle picker">CYCLE 0017 | 0016</button>
            <button data-testid="UIC-DIFF-VIEW-DIAGRAM" className="active">Diagram</button>
            <button data-testid="UIC-DIFF-VIEW-LIST">List</button>
          </div>

          <div className="vf-oclbar">
            <span>OCL file&nbsp;·&nbsp;</span>
            <button data-testid="UIC-OCL-VIEW-SOURCE" className="active">Source</button>
            <button data-testid="UIC-OCL-VIEW-DIAGRAM">Diagram</button>
          </div>
        </main>

        {/* RIGHT — MODEL CONTEXT */}
        <aside className="vf-context">
          <div className="vf-context-head">
            <button data-testid="UIC-CONTEXT-COLLAPSE" title="Collapse or expand the MODEL CONTEXT panel">›</button>
            <span>MODEL CONTEXT</span>
          </div>
          <div className="vf-meta"><b>file</b> current/domain/canvas-view.puml</div>
          <div className="vf-meta"><b>kind</b> domain</div>
          <ul className="vf-ids" data-testid="UIC-CONTEXT-ID-LIST" role="listbox" aria-label="Model ID list">
            <li className="sel">DOM-CANVAS-VIEW</li>
            <li>DOM-PANEL-LAYOUT</li>
          </ul>
          <div className="vf-desc"><b>DESCRIPTION</b><p>Base zoom/pan plus the center-fixed fisheye params.</p></div>
          <div className="vf-trace"><b>TRACE</b><p>SEQ-CANVAS-FISHEYE → depends_on → DOM-CANVAS-VIEW</p></div>
        </aside>
      </div>

      <div className="vf-statusbar">[‹] / [›] chevrons collapse a panel to an 18px sliver; click the sliver to expand</div>
    </div>
  )
}
