# Test ids

The contract between the renderer (lane B) and the T1 scenarios (`tests/e2e/t1`). A scenario addresses an element only by its
`data-testid`; T2 (the packaged runner) addresses elements by accessible name and never by these ids.

Rules:

* One id names one element for one purpose. Lists put the id on each row (`*-row`) and the container (`*-list`).
* Rows that stand for a thing carry its name in a data attribute (`data-name`), so a scenario picks a row by what it shows
  (`[data-testid="source-row"][data-name="Notes"]`), never by position.
* State is read from attributes, not from text: `data-state` on status elements, `data-count` on counters, `data-disabled="true"`
  next to the native `disabled` where a control is not a button.
* An id never changes with the language. Text a scenario needs to read (a count, a name) is in the attribute named below.
* Ids marked **(T1)** are used by a scenario today. A conformance test (`tests/unit/test-ids.test.mjs`) fails when an id marked
  **(T1)** is missing from the renderer sources, so a rename is a visible change here first.
* To add or rename an id: change this file in the same commit as the renderer; do not rename ids a scenario uses without the scenario.

## Shell

| id | element | attributes |
|---|---|---|
| `app-root` **(T1)** | the app's root element, present once the renderer has mounted | `data-screen` = current section (`home` `sources` `ontology` `agents` `settings` `first-run`) |
| `nav-home` `nav-sources` `nav-ontology` `nav-agents` `nav-settings` **(T1)** | the five navigation items | `data-active="true"` on the current one |
| `nav-collapse` | the collapse toggle | |
| `statusbar-engine` **(T1)** | the engine pill in the status bar (click: engine details) | `data-state` = `running` `starting` `unavailable` |
| `statusbar-jobs` **(T1)** | jobs in progress | `data-count` |
| `toast` | a toast | `data-kind` = `info` `success` `error` |
| `dialog` `dialog-confirm` `dialog-cancel` | a modal dialog and its two buttons | |

## First run (volume 25 section 3)

| id | element | attributes |
|---|---|---|
| `screen-first-run` **(T1)** | the whole first-run flow | `data-step` = `welcome` `sources` `understanding` `ontology` `agent` |
| `firstrun-start` **(T1)** | step 1 button: start | |
| `firstrun-progress` | step 1 inline progress while `oo` is located and `oo serve` starts | `data-state` = `locating` `starting` `ready` `error` |
| `firstrun-error` `firstrun-retry` | step 1 failure text and retry | |
| `firstrun-dropzone` **(T1)** | step 2 drop target (my files, no configuration) | |
| `firstrun-pick-files` | step 2: choose files from a dialog (T1 drops files instead: the native dialog has no file purpose) | |
| `firstrun-pick-folder` | step 2: choose a local folder | |
| `firstrun-connect-cloud` `firstrun-connect-database` | step 2: secondary entries | |
| `firstrun-file-row` **(T1)** | step 3: one row per file | `data-name`, `data-state` (the engine's file state) |
| `firstrun-continue` | step 3: leave processing in the background and go on | |
| `firstrun-see-ontology` **(T1)** | step 4: open Ontology | |
| `firstrun-create-token` **(T1)** | step 5: create the read-only token | |
| `firstrun-agent-claude` `firstrun-agent-cursor` `firstrun-agent-codex` `firstrun-agent-generic` | step 5: agent choice | `data-selected` |
| `firstrun-config-text` **(T1)** | step 5: the connection text | |
| `firstrun-config-copy` | step 5: copy it | |
| `firstrun-cli-line` **(T1)** | step 5: the equivalent `oo` command | |
| `firstrun-done` | finish | |

## Home

| id | element | attributes |
|---|---|---|
| `screen-home` **(T1)** | the Home screen | |
| `home-count-sources` `home-count-entities` `home-count-facts` **(T1)** | scale numbers | `data-count` |
| `home-freshness` | freshness line | `data-state` |
| `home-pending` | number waiting for confirmation (links to Ontology review) | `data-count` |
| `home-next-step` | the next-step suggestion card | `data-step` |
| `home-recent-list` `home-recent-row` | recent changes | row: `data-name` |

## Sources

| id | element | attributes |
|---|---|---|
| `screen-sources` **(T1)** | the Sources screen | |
| `source-list` `source-row` **(T1)** | one list for every kind of source | row: `data-name`, `data-kind` = `managed` `folder` `cloud` `database`, `data-state` = `idle` `syncing` `error` |
| `add-source` **(T1)** | opens the add menu | |
| `add-source-files` `add-source-folder` `add-source-cloud` `add-source-database` | menu entries | |
| `add-source-confirm` `add-source-cancel` `add-source-error` | the add form | |
| `screen-source-detail` **(T1)** | one source's page | `data-name` |
| `source-sync` | start a sync | |
| `file-panel` `file-row` **(T1)** | file list of a source | row: `data-name`, `data-state` |
| `file-drop` | drop target inside a managed source | |

## Ontology

| id | element | attributes |
|---|---|---|
| `screen-ontology` **(T1)** | the Ontology screen | |
| `ontology-tab-entities` `ontology-tab-review` `ontology-tab-conflicts` | the three views | `data-active` |
| `entity-search` **(T1)** | search field | |
| `entity-type-filter` | type filter | |
| `entity-list` `entity-row` **(T1)** | entity list | row: `data-name`, `data-type` |
| `screen-entity` **(T1)** | one entity | `data-name` |
| `fact-list` `fact-row` **(T1)** | fact timeline | row: `data-predicate`, `data-state` |
| `evidence-link` **(T1)** | one-step jump to the source anchor | `data-file` |
| `review-list` `review-row` | items waiting for confirmation | |
| `review-accept` `review-reject` | decide the focused item | |
| `constraint-list` `conflict-row` | conflicts | |

## Agents

| id | element | attributes |
|---|---|---|
| `screen-agents` **(T1)** | the Agents screen | |
| `agent-token-create` **(T1)** | opens the create form | |
| `agent-token-name` `agent-token-submit` **(T1)** | the form | |
| `agent-token-secret` **(T1)** | the token, shown once | |
| `agent-token-list` `agent-token-row` `agent-token-revoke` **(T1)** | existing tokens | row: `data-name`, `data-state` = `active` `revoked` |
| `agent-kind-generic` `agent-kind-claude` `agent-kind-cursor` `agent-kind-codex` | agent choice | `data-selected` |
| `agent-config-text` **(T1)** | the connection text | |
| `agent-config-copy` | copy it | |
| `agent-cli-line` | the equivalent `oo` command | |
| `share-list` `share-row` `share-create` `brief-preview` `share-refresh` | context packs | row: `data-name`, `data-state` |
| `audit-list` `audit-row` | access records | |

## Settings

| id | element | attributes |
|---|---|---|
| `screen-settings` **(T1)** | the Settings screen | |
| `settings-section-engine` `settings-section-model` `settings-section-storage` `settings-section-privacy` `settings-section-about` | sections | |
| `engine-status` **(T1)** | engine state text | `data-state` = the engine status (`healthy` `stopped` `starting` `crashed` ...) |
| `engine-start` `engine-stop` `engine-restart` **(T1)** | engine controls | |
| `engine-runtime-select` | runtime choice | `data-value` = `auto` `docker` `lite` |
| `engine-reveal-logs` | show the logs folder | |
| `engine-connect-remote` `remote-url` `remote-token-pick` `remote-connect` `remote-disconnect` | remote engine | |
| `settings-language` | language choice | `data-value` |
| `about-version` **(T1)** | the version line | |
| `about-notice` `about-third-party` `about-license` | legal text | |
