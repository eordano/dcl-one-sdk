# Upstream submission — `migrate-smart-items-to-code`

Target: <https://github.com/decentraland/sdk-skills> (default branch `main`).
Everything below is prepared locally. Nothing has been pushed, forked, or opened.

---

## PR title

```
Add migrate-smart-items-to-code skill: the smart-item palette (63 actions / 19 triggers) in SDK7
```

## Branch name

Their convention for content PRs is `skillwriter/<topic>` (maintainer-run sync branches) or
`feat/<topic>` (`feat/explorer-mcp-skill`). For an outside contribution:
`feat/smart-items-to-code-skill`.

## PR body

> ### What
>
> A new skill, `migrate-smart-items-to-code`, that ports Creator Hub smart items — the
> no-code Actions/Triggers palette interpreted at runtime by `@dcl/asset-packs` — to plain
> SDK7 TypeScript.
>
> Coverage is the whole palette, not a selection: all **63** `ActionType` values, all **19**
> `TriggerType` values, and all **9** `TriggerConditionType` values, each with the minimum
> correct SDK7 call. Counts checked against `@dcl/asset-packs` 2.17.2 (`dist/enums.d.ts`,
> `dist/trigger-enums.d.ts`); snippets type-check against `@dcl/sdk` 7.25.
>
> ```
> migrate-smart-items-to-code/
>   SKILL.md                    437 lines
>   references/actions.md      1122 lines — 63 actions
>   references/triggers.md      414 lines — 19 triggers + 9 conditions
> ```
>
> ### Why a skill and not additions to the existing ones
>
> The reader's task is "this scene was built with smart items and now needs behavior the
> palette can't express" (or "reproduce this smart item in code"). That task needs the whole
> palette in front of it at once — a door is `ON_CLICK → PLAY_SOUND → START_TWEEN →
> SET_STATE`, which under the per-domain split lands in four skills, and the three glue
> concepts the palette supplies (counters, states, trigger conditions) belong to no skill
> because they have no SDK component behind them.
>
> So the skill is deliberately shaped like `migrate-sdk6-to-sdk7`: a porting skill with a
> mapping table, that hands off to the topic skills for every component it touches.
>
> ### It does not duplicate the topic skills
>
> Each section names the skill that owns the component and stops there. `[[add-interactivity]]`
> keeps pointer events and trigger areas; `[[animations-tweens]]` keeps `Animator`/`Tween`;
> `[[audio-video]]`, `[[camera-control]]`, `[[lighting-environment]]`, `[[player-avatar]]`,
> `[[advanced-input]]`, `[[build-ui]]`, `[[scene-runtime]]` likewise. What this skill adds on
> top of them is the palette-specific part:
>
> - which palette entry maps to which API, by name, so an agent reading a Creator Hub scene
>   or a user saying "the Move Player action" lands in the right place;
> - the unit and semantic traps between the two worlds (palette durations are **seconds**,
>   `Tween`/`timers` are **milliseconds**; `START_LOOP` fires immediately, `setInterval`
>   waits one interval; `SET_VISIBILITY` in the palette can also clear colliders);
> - the glue with no component behind it: counters, states, `ON_STATE_CHANGE`, the nine
>   trigger conditions, batching, sequencing, and world-space position for nested
>   Creator Hub hierarchies;
> - the honest "no SDK equivalent" list — `CALL_SCRIPT_METHOD`, `CLAIM_AIRDROP`,
>   `SPAWN_ENTITY`, `CLONE_ENTITY`, `DAMAGE`/`HEAL_PLAYER`, `SHOW_TEXT`/`SHOW_IMAGE` — so
>   an agent says so instead of inventing an API.
>
> Closest existing neighbours, and why neither covers this:
> `[[composites]]` documents the `asset-packs::*` **data** (how the no-code wiring is stored
> and what the Creator Hub manages) — the opposite direction from executing it in code.
> `[[script-components]]` documents writing a Script component class, which is a smart item
> authored in TS *for the editor*, not a scene that has left the editor behind.
>
> ### Registration
>
> Also adds the row to `README.md` and the entry to `sdk-scenes/SKILL.md`, plus one-line
> pointers from the six skills whose domains overlap (see "Files changed").
>
> ### Provenance
>
> Written while building an alternative SDK7 build/preview toolchain; every snippet was
> exercised by building real scenes with it, and the palette coverage was cross-checked
> mechanically against the `@dcl/asset-packs` enums (script in "How to re-verify" below).
> Happy to split it per-domain instead if you'd rather not carry a monolith — but please
> read the "Why a skill" section first, the glue sections are the ones with nowhere else to
> go.

---

## Directory name: `migrate-smart-items-to-code`

Our local name is `dcl-scene-behaviors`, which does not fit their scheme: no repo skill
carries a `dcl-` prefix (the repo *is* Decentraland), and "scene behaviors" names a theme
rather than a task.

Their naming is a task the agent is being asked to do — `add-interactivity`, `build-ui`,
`create-scene`, `deploy-worlds`, `optimize-scene`, `migrate-sdk6-to-sdk7` — with noun names
reserved for reference-shaped skills (`composites`, `camera-control`, `audio-video`).

This skill is task-shaped and specifically porting-shaped, so it takes the same form as the
one porting skill they already have, including the explicit source→target that skill uses:

| Candidate | Verdict |
| --- | --- |
| `migrate-smart-items-to-code` | **Chosen.** Mirrors `migrate-sdk6-to-sdk7` exactly (`migrate-<source>-to-<target>`), keeps "smart-items" as the discovery keyword, and states the direction. |
| `migrate-smart-items` | Shorter, but ambiguous — could read as migrating items between scenes or upgrading asset-pack versions. `migrate-sdk6-to-sdk7` spells out the target for the same reason. |
| `smart-items-in-code` | Noun-phrase, defensible next to `script-components`, but loses the porting verb that makes an agent pick it for a migration task. |
| `replace-smart-items` | Rejected: reads as deprecating a shipping Creator Hub feature. The skill's position is "when the palette runs out", not "don't use smart items". |

If the maintainers prefer a non-`migrate` framing (the skill is also useful for greenfield
code that wants smart-item semantics), `smart-items-in-code` is the drop-in alternative —
only the directory name, the `name:` field, and the cross-reference links change.

---

## Files changed

### New

```
migrate-smart-items-to-code/SKILL.md
migrate-smart-items-to-code/references/actions.md
migrate-smart-items-to-code/references/triggers.md
```

Prepared at `docs/upstream/sdk-skills/migrate-smart-items-to-code/` in this repo; copy the
directory into the root of `sdk-skills` unchanged.

### `README.md` — add to the Available Skills table, between `migrate-sdk6-to-sdk7` and `multiplayer-sync`

```markdown
| `migrate-smart-items-to-code` | Port Creator Hub smart items (63 actions, 19 triggers, 9 conditions) to plain SDK7 TypeScript. |
```

(The table is alphabetical after the `sdk-scenes` / `create-scene` header rows, so this row
sorts directly after `migrate-sdk6-to-sdk7`.)

### `sdk-scenes/SKILL.md` — add to the index, right after the "SDK6 → SDK7 Migration" section

```markdown
### Smart Items → Code

**Skill: `migrate-smart-items-to-code`** — Port the Creator Hub no-code palette to SDK7
TypeScript. All 63 `ActionType`s, 19 `TriggerType`s and 9 `TriggerConditionType`s with SDK7
equivalents, the glue the editor supplied (delays, loops, counters, states, trigger
conditions), the `scene.json` permissions each action needs, and the actions that have no
SDK equivalent.
```

### One-line cross-references into existing skills

Precedent: the `audio-analysis` PR added a pointer in `audio-video/SKILL.md` when it landed.
Suggested, each a single line at the end of the relevant section:

- `add-interactivity/SKILL.md` — after the Trigger Areas section:
  `> Porting a Creator Hub smart item's ON_CLICK / ON_PLAYER_ENTERS_AREA rather than writing one from scratch? See [[migrate-smart-items-to-code]] for the palette-to-SDK7 mapping.`
- `animations-tweens/SKILL.md` — `Smart-item PLAY_ANIMATION / START_TWEEN / SLIDE_TEXTURE / ON_TWEEN_END map here — see [[migrate-smart-items-to-code]].`
- `audio-video/SKILL.md` — `Smart-item PLAY_SOUND / PLAY_AUDIO_STREAM / PLAY_VIDEO_STREAM map here — see [[migrate-smart-items-to-code]].`
- `script-components/SKILL.md` — `To move a smart item out of the editor entirely (no Script component, no asset-packs runtime), see [[migrate-smart-items-to-code]].`
- `composites/SKILL.md` — next to the `asset-packs::Actions` / `Triggers` / `States` rules:
  `To replace that no-code data with TypeScript, see [[migrate-smart-items-to-code]].`
- `migrate-sdk6-to-sdk7/SKILL.md` — in Cross-References:
  `[[migrate-smart-items-to-code]] — if the ported scene is then rebuilt in the Creator Hub with smart items, or already carries asset-packs components`

---

## What a reviewer should check

Ordered by risk. The first four are the ones I would look at if I were reviewing.

1. **Scope overlap is acceptable to you.** This is the decision that makes or breaks the PR.
   Every component section defers to the owning skill and gives only the minimum call for
   that palette entry — but it *is* one file that touches ten domains. If you'd rather the
   trigger half went into `add-interactivity` and only the glue stayed here, say so and I'll
   resplit.

2. **`CL_MAIN_PLAYER` vs `CL_PLAYER` for trigger areas.** `references/triggers.md` recommends
   `TriggerArea.setBox(area, ColliderLayer.CL_MAIN_PLAYER)` for local-player-only zones,
   instead of the default `CL_PLAYER` + `if (result.trigger?.entity !== engine.PlayerEntity) return`
   guard that `add-interactivity` documents and that `@dcl/asset-packs` itself uses. Verified
   the layer exists (`@dcl/ecs` `mesh_collider.gen.d.ts`: `CL_MAIN_PLAYER = 8`, "layer
   corresponding to the local (main) player avatar"). Both are documented in the file, but if
   you confirm the renderer behavior, `add-interactivity` probably wants the same note.

3. **The "don't strip `@dcl/asset-packs` until the composite is clean" rule** in `SKILL.md`.
   It is derived from your own `composites` rules (`asset-packs::Counter` is the id
   allocator; `asset-packs::ActionTypes` and `inspector::*` are Creator Hub-managed and must
   stay). I have not exercised every combination of "remove the import but keep the
   composite data" on current SDKs — please sanity-check the wording against what the
   toolchain actually does today, especially the older-SDK dependency note.

4. **Permission enforcement caveat.** `SKILL.md` says `requiredPermissions` is enforced for
   portable experiences and smart wearables, and that normal parcel/World scenes are not
   currently blocked by their absence — while still telling the agent to declare them. If
   that has changed, this paragraph is the one to fix.

5. **Restricted-action claims:** `openExternalUrl` must be called synchronously from a
   click handler; `teleportTo` / `changeRealm` show a confirmation screen and need no
   permission; `movePlayerTo` / `triggerEmote` only work inside scene bounds. These match
   `scene-runtime` and `player-avatar` — worth a skim for consistency of wording.

6. **`Composite.instance` described as "deprecated/unsupported"** (SPAWN_ENTITY). If there's
   a supported instantiation path now, that entry should point at it instead of the factory
   pattern.

7. **Naming and registration** — see the table above; and whether you want the six
   cross-reference lines in this PR or in a follow-up.

8. **House style:** frontmatter is `name` + `description` only, `description` carries the
   "Use when… / Do NOT use for… (see X)" shape, `{baseDir}/references/…` for reference
   paths, `[[skill]]` for skill links, `## RULE:` / `PITFALL:` markers, `Done when:`
   completion checks, `(verified — <file>)` for source-backed claims. Flag anything that
   reads off-key.

### How to re-verify the coverage claim

Against any scene with `@dcl/asset-packs` installed:

```bash
python3 - <<'PY'
import re
d='node_modules/@dcl/asset-packs/dist/'
def vals(f,name):
    m=re.search(r'enum %s \{(.*?)\n\}'%name, open(d+f).read(), re.S)
    return re.findall(r'^\s+([A-Z0-9_]+)\s*=', m.group(1), re.M)
acts=vals('enums.d.ts','ActionType')
trigs=vals('trigger-enums.d.ts','TriggerType')
conds=vals('trigger-enums.d.ts','TriggerConditionType')
doc=open('migrate-smart-items-to-code/references/actions.md').read() + \
    open('migrate-smart-items-to-code/references/triggers.md').read()
print(len(acts), len(trigs), len(conds))
print('missing:', [x for x in acts+trigs+conds if x not in doc])
PY
```

Expected: `63 19 9` and `missing: []`.

---

## Their process, for whoever opens the PR

- **No CONTRIBUTING file, no CI, no tests.** README's Contributing section is one line:
  open a PR in this repo. Every merge in the history is a plain squash-free merge commit by
  `nearnshaw` (Nico Earnshaw), who is effectively the sole maintainer.
- **The repo is downstream of `decentraland/docs`.** The initial import commit says the
  skills were copied from the `skills/` directory of `decentraland/docs` "for fast
  installation via the Vercel skills CLI". A change that also belongs in the docs repo may
  need to be made there too; ask in the PR.
- **`.sync-state.json` is a maintainer bookkeeping file — do not touch it.** It records the
  last reviewed commit of six source repos (`docs`, `protocol`, `js-sdk-toolchain`,
  `creator-hub`, `sdk7-test-scenes`, `sdk-skills`) with a `lastChecked` date. A recurring
  "skill sync" pass (branches `skillwriter/sync-<date>`, roughly every 1–2 weeks; latest
  2026-07-27) walks the commits in those repos since the recorded state, folds anything
  skill-relevant into the skills, and advances the file. Consequence for a submitter: a new
  skill may be rewritten by a later sync pass, and claims should be phrased so a sync can
  re-verify them — which is why their style demands `(verified — <file>)` and named test
  scenes.
- **`sdk7-test-scenes` is the preferred evidence.** Several skills end with an "Example
  scenes" list linking to `github.com/decentraland/sdk7-test-scenes/tree/main/scenes/<coords>-<name>`,
  and inline claims cite the scene that proved them. This skill cites the package enums and
  `@dcl/sdk` typings instead, because the palette lives in `@dcl/asset-packs` and there is
  no smart-item scene in that repo. Offering to add one would strengthen the PR.
- **Install path is the Vercel skills CLI** (`npx skills add decentraland/sdk-skills --all`),
  which reads each top-level directory as a skill. A new top-level directory is picked up
  with no manifest to edit — but it will be invisible in the README table and the
  `sdk-scenes` index unless you add it there (`unity-explorer-mcp` is currently in the repo
  and in neither, which is what that omission looks like).
- **`.claude/settings.json` is the maintainer's own machine config** (a permission allowlist
  with absolute paths under the maintainer's own home directory). It is not contributor
  guidance; leave it
  alone.
- **Recent direction of travel**, worth matching: PR #61 pruned skill descriptions and folded
  trailing summary sections; the same series added `Done when:` completion checks to
  `create-scene` and `migrate-sdk6-to-sdk7` and fixed "progressive disclosure" across seven
  skills (SKILL.md stays lean, detail moves into `references/`). Submitting a lean SKILL.md
  with two fat references is with the grain.
