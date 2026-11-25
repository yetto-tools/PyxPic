# PyPIC IDE Design System

## 0. Research Log

- Concrete reference: the supplied ESP-IDF extension flow and its installation, project and toolbar states. We retain its visible onboarding path (welcome → tools → project → build) without copying its branding or surface.
- Embedded reference: VS Code theme variables and workbench controls are the host system. The extension uses their native colours and typography so it reads as part of the editor.
- Skipped lanes: a standalone web research lane and image drafts are not applicable; this is a native VS Code webview with a concrete host and reference.

## 1. Atmosphere & Identity

PyPIC IDE is a calm workbench for firmware: clear about what is configured, explicit about what can happen next, and quiet when no project exists. Its signature is a practical start page that pairs one primary project action with the actual state of the compiler and Device Packs.

## 2. Color

All webview colour tokens resolve to VS Code theme variables. The host owns light and dark palettes.

| Role | Token | Usage |
|---|---|---|
| Surface/primary | `--surface-primary` | Webview background |
| Surface/secondary | `--surface-secondary` | Group background |
| Surface/elevated | `--surface-elevated` | Dialog-like action area |
| Text/primary | `--text-primary` | Headings and normal text |
| Text/secondary | `--text-secondary` | Supporting copy |
| Border/default | `--border-default` | Rules and controls |
| Accent/primary | `--accent-primary` | One primary action and focus |
| Status/success | `--status-success` | Detected usable tool |
| Status/warning | `--status-warning` | Missing configuration |
| Status/error | `--status-error` | Failed action |

## 3. Typography

The webview uses `var(--vscode-font-family)`, then the operating system sans-serif fallback. Code, paths and compiler output use `var(--vscode-editor-font-family)`.

| Level | Size | Weight | Usage |
|---|---:|---:|---|
| H1 | 28px | 650 | Welcome title |
| H2 | 18px | 650 | Section heading |
| Body | 14px | 400 | Description and labels |
| Body/sm | 12px | 400 | Status explanation |
| Code | 12px | 400 | Tool and path values |

## 4. Spacing & Layout

Base unit: 4px. Tokens are `--space-1` through `--space-10`, mapped in the webview stylesheet. The welcome screen owns its vertical scroll. At 720px and above it uses a two-column grid: the project action occupies the first reading position and the environment state is adjacent. Below that it becomes one column. The activity bar uses one native VS Code tree whose collapsed roots are **Proyecto**, **Construir** and **Entorno**.

Layout: a scroll-owned welcome webview with a fixed-width readable content rail; project actions first, environment status second, supported next steps last; the webview never causes the VS Code workbench itself to scroll.

## 5. Components

### Welcome action
- **Structure:** heading, factual sentence, one primary button, quiet secondary action.
- **States:** default, hover, pressed, focus-visible, disabled while a command runs, error in-place.
- **Accessibility:** buttons use native `<button>`, reading order matches visual order, visible focus ring.

### Tool status row
- **Structure:** name, current value, state and one contextual action.
- **States:** ready, missing, unknown and error.
- **Accessibility:** status text is explicit, colour is supplementary only, action has a descriptive label and the visible secondary-button treatment.

### Project starter
- **Structure:** project name/state, short source template note, action pair.
- **States:** no folder, ready to create, created, existing target error, cancelled.
- **Accessibility:** creation uses native VS Code input and folder dialogs; the webview only starts the flow.

### Activity-bar section
- **Structure:** a native VS Code tree with collapsed roots for Proyecto, Construir and Entorno; expanding a root reveals only actions from that task.
- **States:** collapsed and expanded; each action remains available from the Command Palette.
- **Accessibility:** native tree view semantics, keyboard navigation and theme icons supplied by VS Code.

## 6. Motion & Interaction

Controls use native-colour hover and pressed changes with no layout animation. A pending action replaces its label without changing button width. `prefers-reduced-motion` disables transitions.

## 7. Depth & Surface

Strategy: borders-only. A neutral 1px divider groups sections; tonal background shifts separate the environment area. No shadows and no nested cards.

## 8. Accessibility Constraints & Accepted Debt

- Keyboard reachable controls, native semantic elements and VS Code theme contrast are required.
- The welcome flow is usable in restricted workspaces: it explains that project creation and compilation require trust.
- Accepted debt: no physical programmer, serial terminal, simulation panel or debugger exists yet. The screen states this as unavailable rather than offering a dead action.

## 9. Ideal State & Gap Ledger

Personas: a learner creating a first PIC program; a firmware developer reopening an existing project. Journey steps: first visit, tool recovery, project creation, daily build.

| ID | Screen / state | Gap | Fix | Proof | Status |
|---|---|---|---|---|---|
| IDE-1 | First visit | No place that explains the next usable action | Welcome page with new project and example actions | Webview manual QA | Planned |
| IDE-2 | Tool recovery | Compiler and Device Pack paths were hidden in settings | Environment rows and path pickers | Command tests and webview QA | Planned |
| IDE-3 | Project creation | No guided source template | Native VS Code project wizard | Command tests and created files | Planned |
| IDE-4 | Activity bar | All actions appeared in one unscannable list | Separate Project, Build and Environment views | Extension UI QA and command tests | Planned |
