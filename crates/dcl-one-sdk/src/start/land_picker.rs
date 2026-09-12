//! The LAND picker: every parcel the wallet owns or operates, clustered into
//! contiguous areas by a four-neighbour walk, each area saying how empty it
//! is and when it was last published to, so choosing where a scene goes is a
//! click on an area rather than a trip to the marketplace. Past three areas
//! the list splits into three columns — emptier, newest deployed to, older
//! deployed to — because those are the three reasons to pick one. It stays
//! folded only while the scene already sits, published, on parcels the
//! wallet holds; a scene that has never been on LAND (or came back from a
//! World) gets it open.

use std::collections::{HashMap, HashSet, VecDeque};

use super::chrome::esc;
use super::deploy_page::{connect_buttons, footprint, note_span, short_addr};
use super::deploy_rights::{land_read_bases, LandScene, LandUse, Rights, HOLDINGS_PAGE};
use super::deploy_status::{ago, host_of, plural, Dest, LiveStatus, Remote, RemoteState};
use crate::deploy;

/// Rows per column past three areas; one list shows [`LIST_CAP`].
const COLUMN_CAP: usize = 6;
const LIST_CAP: usize = 12;

/// A contiguous group of the wallet's parcels and what sits on them.
pub(super) struct Area {
    /// Sorted by x then y, so every ranking is deterministic.
    pub(super) parcels: Vec<(i64, i64)>,
    pub(super) empty: usize,
    pub(super) scenes: usize,
    pub(super) newest: Option<i64>,
    pub(super) oldest: Option<i64>,
    /// Titles of the scenes here, newest first, at most three.
    pub(super) titles: Vec<String>,
    /// Where the declared footprint fits inside this area with the fewest
    /// parcels already carrying a scene, if it fits at all.
    pub(super) fit: Option<((i64, i64), usize)>,
    /// The declared footprint already overlaps this area.
    pub(super) holds_scene: bool,
}

/// Four-neighbour clustering of `coords`, each cluster scored against what
/// `land_use` says is deployed and against the declared footprint (`declared`
/// relative to `base`), which the picker tries to fit inside every area.
pub(super) fn areas(
    coords: &[(i64, i64)],
    land_use: Option<&LandUse>,
    declared: &[(i64, i64)],
    base: (i64, i64),
) -> Vec<Area> {
    let held: HashSet<(i64, i64)> = coords.iter().copied().collect();
    let scenes: &[LandScene] = land_use.map(|u| u.scenes.as_slice()).unwrap_or_default();
    let mut on: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in scenes.iter().enumerate() {
        for c in &s.coords {
            on.entry(*c).or_default().push(i);
        }
    }
    let shape: Vec<(i64, i64)> = declared
        .iter()
        .map(|d| (d.0 - base.0, d.1 - base.1))
        .collect();
    let mut ordered: Vec<(i64, i64)> = held.iter().copied().collect();
    ordered.sort_unstable();
    let mut seen: HashSet<(i64, i64)> = HashSet::new();
    let mut out = Vec::new();
    for &start in &ordered {
        if !seen.insert(start) {
            continue;
        }
        let mut parcels = vec![start];
        let mut queue = VecDeque::from([start]);
        while let Some((x, y)) = queue.pop_front() {
            for n in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                if held.contains(&n) && seen.insert(n) {
                    parcels.push(n);
                    queue.push_back(n);
                }
            }
        }
        parcels.sort_unstable();
        let set: HashSet<(i64, i64)> = parcels.iter().copied().collect();
        let mut here: Vec<usize> = parcels
            .iter()
            .filter_map(|p| on.get(p))
            .flatten()
            .copied()
            .collect();
        here.sort_unstable();
        here.dedup();
        let mut stamped: Vec<&LandScene> = here.iter().map(|&i| &scenes[i]).collect();
        stamped.sort_by_key(|s| std::cmp::Reverse(s.timestamp));
        let mut titles: Vec<String> = Vec::new();
        for s in &stamped {
            if !titles.contains(&s.title) {
                titles.push(s.title.clone());
            }
            if titles.len() == 3 {
                break;
            }
        }
        let fit = parcels
            .iter()
            .filter_map(|&cand| {
                let placed: Vec<(i64, i64)> =
                    shape.iter().map(|s| (cand.0 + s.0, cand.1 + s.1)).collect();
                placed
                    .iter()
                    .all(|p| set.contains(p))
                    .then(|| (cand, placed.iter().filter(|p| on.contains_key(p)).count()))
            })
            .min_by_key(|(_, overlap)| *overlap);
        out.push(Area {
            empty: parcels.iter().filter(|p| !on.contains_key(p)).count(),
            scenes: here.len(),
            newest: stamped.iter().filter_map(|s| s.timestamp).max(),
            oldest: stamped.iter().filter_map(|s| s.timestamp).min(),
            titles,
            fit,
            holds_scene: declared.iter().any(|d| set.contains(d)),
            parcels,
        });
    }
    out
}

pub(super) struct Column<'a> {
    pub(super) head: &'static str,
    pub(super) areas: Vec<&'a Area>,
    /// Areas the cap dropped, so a capped column never reads as complete.
    pub(super) more: usize,
}

/// The area the scene sits on, pinned above the rankings — it is never a
/// move, so it never competes for a column.
pub(super) fn here(areas: &[Area]) -> Vec<&Area> {
    areas.iter().filter(|a| a.holds_scene).collect()
}

fn one_list(mut list: Vec<&Area>) -> Vec<Column<'_>> {
    let more = list.len().saturating_sub(LIST_CAP);
    list.truncate(LIST_CAP);
    vec![Column {
        head: "Your LAND",
        areas: list,
        more,
    }]
}

/// The areas a move could land on. One list while they all fit the emptier
/// column (or while nothing is deployed on any, when the two age columns
/// would be empty); past that, three columns that never repeat an area —
/// each takes what the columns before it did not show, a column with
/// nothing left is dropped, and a lone survivor folds back into the list.
pub(super) fn columns(areas: &[Area]) -> Vec<Column<'_>> {
    let rest: Vec<&Area> = areas.iter().filter(|a| !a.holds_scene).collect();
    let mut by_empty = rest.clone();
    by_empty.sort_by_key(|a| {
        (
            std::cmp::Reverse(a.empty),
            std::cmp::Reverse(a.parcels.len()),
            a.parcels[0],
        )
    });
    let deployed = rest.iter().filter(|a| a.newest.is_some()).count();
    if rest.len() <= COLUMN_CAP || deployed == 0 {
        return one_list(by_empty);
    }
    let mut newest: Vec<&Area> = rest
        .iter()
        .copied()
        .filter(|a| a.newest.is_some())
        .collect();
    newest.sort_by_key(|a| (std::cmp::Reverse(a.newest), a.parcels[0]));
    let mut oldest: Vec<&Area> = rest
        .iter()
        .copied()
        .filter(|a| a.oldest.is_some())
        .collect();
    oldest.sort_by_key(|a| (a.oldest, a.parcels[0]));
    let mut shown: HashSet<(i64, i64)> = HashSet::new();
    let picked: Vec<(&'static str, Vec<&Area>, Vec<&Area>)> = [
        ("Emptier", by_empty.clone()),
        ("Newest deployed to", newest),
        ("Older deployed to", oldest),
    ]
    .into_iter()
    .map(|(head, ranked)| {
        let take: Vec<&Area> = ranked
            .iter()
            .copied()
            .filter(|a| !shown.contains(&a.parcels[0]))
            .take(COLUMN_CAP)
            .collect();
        shown.extend(take.iter().map(|a| a.parcels[0]));
        (head, ranked, take)
    })
    .collect();
    let cols: Vec<Column<'_>> = picked
        .into_iter()
        .filter(|(_, _, take)| !take.is_empty())
        .map(|(head, ranked, take)| Column {
            head,
            more: ranked
                .iter()
                .filter(|a| !shown.contains(&a.parcels[0]))
                .count(),
            areas: take,
        })
        .collect();
    if cols.len() < 2 {
        return one_list(by_empty);
    }
    cols
}

/// Folded only when the scene already sits, published, on parcels the wallet
/// holds — the one case where picking a new area is not the next step.
pub(super) fn folded(
    declared: &[(i64, i64)],
    held: &HashSet<(i64, i64)>,
    land_use: Option<&LandUse>,
    remote: &Remote,
    land_target: bool,
) -> bool {
    if declared.is_empty() || !declared.iter().all(|d| held.contains(d)) {
        return false;
    }
    match land_use {
        Some(u) => u
            .scenes
            .iter()
            .any(|s| s.coords.iter().any(|c| declared.contains(c))),
        None => {
            land_target
                && matches!(
                    remote,
                    Remote::Known(RemoteState {
                        current: Some(_),
                        ..
                    })
                )
        }
    }
}

pub(super) fn land_picker(
    prefix: &str,
    tok: &str,
    dest: &Dest,
    rights: Option<&Rights>,
    status: &LiveStatus,
) -> String {
    let (declared, base) = footprint(dest);
    let Some(r) = rights else {
        return wrap(
            true,
            "Your LAND",
            &format!(
                r#"<span class="note">Connect an account to list the parcels it owns or operates, grouped into areas this scene can move to.</span>{}"#,
                connect_buttons(prefix, tok)
            ),
            "",
        );
    };
    let Some(h) = r.holdings.as_ref() else {
        return wrap(
            true,
            "Your LAND",
            &note_span(&format!(
                "Could not list the LAND of {} \u{2014} {} did not answer. Reload to try again.",
                short_addr(&r.address),
                host_of(&dest.chain_lambdas)
            )),
            "",
        );
    };
    if h.coords.is_empty() {
        let estates = match h.estates {
            0 => String::new(),
            n => format!(
                " Parcels inside its {n} estate{} are not listed: the lambdas return estates without coordinates.",
                plural(n as usize)
            ),
        };
        return wrap(
            true,
            "Your LAND",
            &note_span(&format!(
                "{} holds no parcels and operates none.{estates}",
                short_addr(&r.address)
            )),
            "",
        );
    }
    let held: HashSet<(i64, i64)> = h.coords.iter().copied().collect();
    let land_target = dest.world.is_none();
    let known = r.land_use.is_some();
    let open = !folded(
        &declared,
        &held,
        r.land_use.as_ref(),
        &status.remote,
        land_target,
    );
    let areas = areas(&h.coords, r.land_use.as_ref(), &declared, base);
    let empties: usize = areas.iter().map(|a| a.empty).sum();
    let cols = columns(&areas);
    let one = cols.len() == 1;
    let now = deploy::now_ms();
    let here_html: String = match here(&areas).as_slice() {
        [] => String::new(),
        pinned => format!(
            r#"<div class="tgt__picker-here"><div class="wl">{}</div></div>"#,
            pinned
                .iter()
                .map(|a| row(prefix, tok, a, declared.len(), now, known))
                .collect::<String>()
        ),
    };
    let cols_html: String = cols
        .iter()
        .map(|c| {
            let rows: String = c
                .iter_rows()
                .map(|a| row(prefix, tok, a, declared.len(), now, known))
                .collect();
            let more = match c.more {
                0 => String::new(),
                n => format!(
                    r#"<span class="note">and {n} more area{}</span>"#,
                    plural(n)
                ),
            };
            let head = if one {
                String::new()
            } else {
                format!(r#"<span class="knob__k">{}</span>"#, c.head)
            };
            format!(
                r#"<div class="tgt__picker-col">{head}<div class="wl">{rows}</div>{more}</div>"#
            )
        })
        .collect();
    let mut notes: Vec<String> = Vec::new();
    match r.land_use.as_ref() {
        None => notes.push(format!(
            "What is on these parcels could not be read from {}, so no area counts as empty here.",
            host_of(&land_read_bases(dest)[0])
        )),
        Some(u) => notes.extend(u.note.clone()),
    }
    if h.owned.len() >= HOLDINGS_PAGE || h.operated_coords.len() >= HOLDINGS_PAGE {
        notes.push(format!(
            "Only the first {HOLDINGS_PAGE} owned and the first {HOLDINGS_PAGE} operated parcels are listed."
        ));
    }
    if h.estates > 0 {
        notes.push(format!(
            "Parcels inside {} estate{} are not listed: the lambdas return estates without coordinates.",
            h.estates,
            plural(h.estates as usize)
        ));
    }
    if !land_target {
        notes.push(
            "Move here sets this scene's base parcel; Select LAND then makes Genesis City the target."
                .to_string(),
        );
    }
    let summary = if known {
        format!(
            "Your LAND \u{b7} {} area{} \u{b7} {empties} empty parcel{} of {}",
            areas.len(),
            plural(areas.len()),
            plural(empties),
            h.coords.len()
        )
    } else {
        format!(
            "Your LAND \u{b7} {} area{} \u{b7} {} parcel{}",
            areas.len(),
            plural(areas.len()),
            h.coords.len(),
            plural(h.coords.len())
        )
    };
    wrap(
        open,
        &summary,
        &format!(
            r#"{here_html}<div class="tgt__picker-cols{}">{cols_html}</div>"#,
            if one { " tgt__picker-cols--one" } else { "" }
        ),
        &notes.iter().map(|n| note_span(n)).collect::<String>(),
    )
}

impl<'a> Column<'a> {
    fn iter_rows(&self) -> impl Iterator<Item = &'a Area> + '_ {
        self.areas.iter().copied()
    }
}

fn row(prefix: &str, tok: &str, a: &Area, footprint: usize, now: i64, known: bool) -> String {
    let anchor = a.fit.map(|(b, _)| b).unwrap_or(a.parcels[0]);
    let n = a.parcels.len();
    let mut bits = vec![format!("{n} parcel{}", plural(n))];
    if known {
        bits.push(format!("{} empty", a.empty));
        match a.newest {
            Some(ts) => {
                bits.push(format!("last published {}", ago(ts, now)));
                if let Some(t) = a.titles.first() {
                    bits.push(esc(t));
                }
                if a.scenes > 1 {
                    bits.push(format!("{} scenes", a.scenes));
                }
            }
            None => bits.push("nothing deployed".to_string()),
        }
    }
    match a.fit {
        Some((_, 0)) => {}
        Some(_) if a.holds_scene => {}
        Some((_, k)) => bits.push(format!("covers {k} parcel{} with a scene", plural(k))),
        None => bits.push(format!("too small for this {footprint}-parcel scene")),
    }
    let action = if a.holds_scene {
        r#"<span class="tgt__badge">Scene is here</span>"#.to_string()
    } else {
        format!(
            r#"<form method="post" action="{}/target/base"><input type="hidden" name="token" value="{}"><input type="hidden" name="base" value="{x},{y}"><button class="deep__copy" type="submit">Move here</button></form>"#,
            esc(prefix),
            esc(tok),
            x = anchor.0,
            y = anchor.1
        )
    };
    format!(
        r#"<div class="wl__r"><span class="tgt__coord">{},{}</span><span class="wl__d">{}</span>{action}</div>"#,
        anchor.0,
        anchor.1,
        bits.join(" \u{b7} ")
    )
}

fn wrap(open: bool, summary: &str, body: &str, notes: &str) -> String {
    format!(
        r#"<details class="tgt__picker"{}><summary>{}</summary>{body}{notes}</details>"#,
        if open { " open" } else { "" },
        esc(summary)
    )
}

#[cfg(test)]
mod tests {
    use super::super::deploy_rights::Holdings;
    use super::super::deploy_status::{resolve_dest, CurrentScene};
    use super::*;
    use serde_json::json;

    const ADDR: &str = "0x1111111111111111111111111111111111111111";

    fn scene(title: &str, ts: i64, coords: &[(i64, i64)]) -> LandScene {
        LandScene {
            title: title.to_string(),
            timestamp: Some(ts),
            coords: coords.to_vec(),
        }
    }

    fn holdings(coords: &[(i64, i64)]) -> Holdings {
        Holdings {
            parcels: coords.len() as i64,
            estates: 0,
            operated: 0,
            coords: coords.to_vec(),
            owned: coords.to_vec(),
            operated_coords: Vec::new(),
        }
    }

    fn dest(parcels: &[&str], base: &str) -> Dest {
        resolve_dest(
            &json!({ "scene": { "parcels": parcels, "base": base } }),
            None,
            None,
        )
    }

    #[test]
    fn areas_are_four_neighbour_islands() {
        let coords = [(0, 0), (1, 0), (0, 1), (5, 5), (6, 6)];
        let got = areas(&coords, None, &[], (0, 0));
        let sizes: Vec<usize> = got.iter().map(|a| a.parcels.len()).collect();
        assert_eq!(sizes, [3, 1, 1], "{sizes:?}");
        assert_eq!(got[0].parcels, [(0, 0), (0, 1), (1, 0)]);
    }

    /// Empties, ages and titles come from what is deployed; a parcel under
    /// no active entity is empty, and the newest scene names the area.
    #[test]
    fn empties_and_ages_come_from_what_is_deployed() {
        let coords = [(0, 0), (1, 0), (2, 0), (9, 9)];
        let land_use = LandUse {
            scenes: vec![
                scene("Old plaza", 1_000, &[(0, 0)]),
                scene("New plaza", 5_000, &[(1, 0)]),
            ],
            note: None,
        };
        let got = areas(&coords, Some(&land_use), &[], (0, 0));
        let strip = &got[0];
        assert_eq!((strip.empty, strip.scenes), (1, 2));
        assert_eq!((strip.newest, strip.oldest), (Some(5_000), Some(1_000)));
        assert_eq!(strip.titles, ["New plaza", "Old plaza"]);
        let lone = &got[1];
        assert_eq!((lone.empty, lone.scenes, lone.newest), (1, 0, None));
    }

    /// A two-parcel footprint on a 2×2 area whose top row carries a scene
    /// lands on the bottom row; an area smaller than the footprint has no fit.
    #[test]
    fn the_fit_prefers_parcels_nothing_is_deployed_on() {
        let coords = [(10, 10), (11, 10), (10, 11), (11, 11), (50, 50)];
        let land_use = LandUse {
            scenes: vec![scene("Top", 1, &[(10, 11), (11, 11)])],
            note: None,
        };
        let declared = [(0, 0), (1, 0)];
        let got = areas(&coords, Some(&land_use), &declared, (0, 0));
        assert_eq!(got[0].fit, Some(((10, 10), 0)), "{:?}", got[0].fit);
        assert_eq!(got[1].fit, None);
        let cramped = areas(&coords[..2], Some(&land_use), &declared, (0, 0));
        assert_eq!(cramped[0].fit, Some(((10, 10), 0)));
    }

    /// Everything that fits the emptier column is one list, emptiest first;
    /// so is a wallet on which nothing is known to be deployed.
    #[test]
    fn one_list_while_everything_fits_the_emptier_column() {
        let land_use = LandUse {
            scenes: vec![scene("A", 100, &[(0, 0)]), scene("B", 900, &[(5, 0)])],
            note: None,
        };
        let three = [(0, 0), (5, 0), (9, 0)];
        let got = areas(&three, Some(&land_use), &[], (0, 0));
        let cols = columns(&got);
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].head, "Your LAND");
        assert_eq!(cols[0].areas[0].parcels, [(9, 0)], "emptiest first");

        let five = [(0, 0), (5, 0), (9, 0), (20, 0), (21, 0), (30, 0)];
        let got = areas(&five, Some(&land_use), &[], (0, 0));
        assert_eq!(got.len(), 5);
        let cols = columns(&got);
        assert_eq!(cols.len(), 1, "five areas fit one list");
        let order: Vec<(i64, i64)> = cols[0].areas.iter().map(|a| a.parcels[0]).collect();
        assert_eq!(
            order,
            [(20, 0), (9, 0), (30, 0), (0, 0), (5, 0)],
            "two empties, then one, then none"
        );

        let quiet = areas(&five, None, &[], (0, 0));
        assert_eq!(
            columns(&quiet).len(),
            1,
            "nothing known to be deployed: one list"
        );
    }

    /// Past the cap: the scene's own area is pinned and never ranked, an
    /// area shows in the first column that wants it so none repeats, `more`
    /// counts only what no column shows, and a column left with nothing is
    /// dropped — down to one list when only the emptier column survives.
    #[test]
    fn three_columns_past_the_cap_never_repeat_an_area() {
        let mut coords = vec![(200, 0), (201, 0)];
        coords.extend((0..7).map(|i| (100 + 10 * i, 0)));
        coords.extend((0..8).map(|i| (10 * i, 0)));
        let mut scenes = vec![scene("Home", 9_000, &[(200, 0), (201, 0)])];
        scenes.extend((0..8).map(|i| scene("S", 1_000 * (i + 1), &[(10 * i, 0)])));
        let land_use = LandUse { scenes, note: None };
        let got = areas(&coords, Some(&land_use), &[(200, 0), (201, 0)], (200, 0));
        assert_eq!(got.len(), 16);
        let pinned = here(&got);
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].parcels, [(200, 0), (201, 0)]);

        let cols = columns(&got);
        let heads: Vec<&str> = cols.iter().map(|c| c.head).collect();
        assert_eq!(
            heads,
            ["Emptier", "Newest deployed to", "Older deployed to"]
        );
        let mut seen: Vec<(i64, i64)> = Vec::new();
        for c in &cols {
            for a in &c.areas {
                assert!(!seen.contains(&a.parcels[0]), "{:?} repeats", a.parcels);
                assert!(!a.holds_scene, "the scene's area is not a move");
                seen.push(a.parcels[0]);
            }
        }
        assert!(
            cols[0].areas.iter().all(|a| a.newest.is_none()),
            "the seven empties outrank every deployed area"
        );
        assert_eq!(
            (cols[0].areas.len(), cols[0].more),
            (6, 1),
            "one empty hidden"
        );
        let newest: Vec<(i64, i64)> = cols[1].areas.iter().map(|a| a.parcels[0]).collect();
        assert_eq!(
            newest,
            [(70, 0), (60, 0), (50, 0), (40, 0), (30, 0), (20, 0)]
        );
        assert_eq!(cols[1].more, 0, "the two it dropped show under Older");
        let older: Vec<(i64, i64)> = cols[2].areas.iter().map(|a| a.parcels[0]).collect();
        assert_eq!(older, [(0, 0), (10, 0)]);
        assert_eq!(cols[2].more, 0);

        let six = [(0, 0), (10, 0), (20, 0), (30, 0), (40, 0), (50, 0), (60, 0)];
        let land_use = LandUse {
            scenes: (0..7)
                .map(|i| scene("S", 1_000 * (i + 1), &[(10 * i, 0)]))
                .collect(),
            note: None,
        };
        let got = areas(&six, Some(&land_use), &[(0, 0)], (0, 0));
        let cols = columns(&got);
        assert_eq!(
            cols.len(),
            1,
            "{:?}",
            cols.iter().map(|c| c.head).collect::<Vec<_>>()
        );
        assert_eq!(cols[0].head, "Your LAND");
        assert_eq!(cols[0].areas.len(), 6, "the pinned area is not in the list");
    }

    /// Folded only for a published scene on held parcels: not when the
    /// parcels belong to someone else, not when nothing is deployed there,
    /// and never when the footprint is empty.
    #[test]
    fn folds_only_over_a_published_scene_on_held_land() {
        let held: HashSet<(i64, i64)> = [(0, 0), (1, 0)].into_iter().collect();
        let declared = [(0, 0), (1, 0)];
        let on_it = LandUse {
            scenes: vec![scene("Here", 1, &[(0, 0), (1, 0)])],
            note: None,
        };
        let elsewhere = LandUse {
            scenes: vec![scene("There", 1, &[(7, 7)])],
            note: None,
        };
        let unknown = Remote::Unknown("dry run".into());
        assert!(folded(&declared, &held, Some(&on_it), &unknown, true));
        assert!(!folded(&declared, &held, Some(&elsewhere), &unknown, true));
        assert!(!folded(&[(3, 3)], &held, Some(&on_it), &unknown, true));
        assert!(!folded(&[], &held, Some(&on_it), &unknown, true));
        let live = Remote::Known(RemoteState {
            current: Some(CurrentScene {
                title: "Here".into(),
                timestamp: Some(1),
                parcels: 2,
                coords: declared.to_vec(),
                size: None,
            }),
            others: Vec::new(),
            hashes: HashSet::new(),
        });
        assert!(
            folded(&declared, &held, None, &live, true),
            "no land read: the live status decides"
        );
        assert!(
            !folded(&declared, &held, None, &live, false),
            "a world's status says nothing about LAND"
        );
    }

    #[test]
    fn the_picker_renders_moves_and_the_badge() {
        let coords = [
            (2, 12),
            (3, 12),
            (20, -4),
            (21, -4),
            (40, 0),
            (50, 0),
            (60, 0),
        ];
        let land_use = LandUse {
            scenes: vec![
                scene("Hexabricks", 4_000, &[(2, 12), (3, 12)]),
                scene("Kiosk", 2_000, &[(40, 0)]),
            ],
            note: None,
        };
        let rights = Rights {
            holdings: Some(holdings(&coords)),
            land_use: Some(land_use),
            ..Rights::unchecked(ADDR, "")
        };
        let d = dest(&["2,12", "3,12"], "2,12");
        let html = land_picker(
            "/t/demo",
            "tok",
            &d,
            Some(&rights),
            &LiveStatus::unknown("dry run"),
        );
        assert!(html.starts_with(r#"<details class="tgt__picker"><summary>Your LAND · 5 areas · 4 empty parcels of 7</summary><div class="tgt__picker-here"><div class="wl"><div class="wl__r"><span class="tgt__coord">2,12</span>"#), "the scene's area is pinned above the rankings: {html}");
        assert!(
            html.contains("tgt__picker-cols--one")
                && !html.contains("Emptier")
                && !html.contains("Newest deployed to"),
            "one deployed area left to rank: one list, not three columns: {html}"
        );
        assert!(html.contains(r#"<span class="tgt__coord">2,12</span><span class="wl__d">2 parcels · 0 empty · last published"#), "{html}");
        assert_eq!(
            html.matches("2 parcels · 0 empty").count(),
            1,
            "pinned once, ranked nowhere: {html}"
        );
        assert!(
            html.contains("Hexabricks</span><span class=\"tgt__badge\">Scene is here</span>"),
            "{html}"
        );
        assert!(html.contains(r#"name="base" value="20,-4"><button class="deep__copy" type="submit">Move here</button>"#), "{html}");
        assert!(
            html.contains("1 parcel · 0 empty · last published") && html.contains("Kiosk"),
            "{html}"
        );
        assert!(html.contains("too small for this 2-parcel scene"), "{html}");

        let moved = dest(&["7,7", "8,7"], "7,7");
        let html = land_picker(
            "/t/demo",
            "tok",
            &moved,
            Some(&rights),
            &LiveStatus::unknown("dry run"),
        );
        assert!(
            html.starts_with(r#"<details class="tgt__picker" open>"#),
            "off held land: open: {html}"
        );
        assert!(!html.contains("Scene is here"), "{html}");
    }

    /// Without an account the block asks for one; without holdings it says
    /// who did not answer; with none it says so and explains estates.
    #[test]
    fn the_picker_explains_what_it_cannot_list() {
        let d = dest(&["0,0"], "0,0");
        let live = LiveStatus::unknown("dry run");
        let html = land_picker("", "tok", &d, None, &live);
        assert!(
            html.contains("Connect an account") && html.contains("data-wallet"),
            "{html}"
        );
        let html = land_picker("", "tok", &d, Some(&Rights::unchecked(ADDR, "")), &live);
        assert!(html.contains("Could not list the LAND of 0x1111\u{2026}1111 \u{2014} peer.decentraland.org did not answer"), "{html}");
        let none = Rights {
            holdings: Some(Holdings {
                estates: 2,
                ..holdings(&[])
            }),
            ..Rights::unchecked(ADDR, "")
        };
        let html = land_picker("", "tok", &d, Some(&none), &live);
        assert!(
            html.contains(
                "holds no parcels and operates none. Parcels inside its 2 estates are not listed"
            ),
            "{html}"
        );
        let unread = Rights {
            holdings: Some(holdings(&[(0, 0), (1, 0)])),
            land_use: None,
            ..Rights::unchecked(ADDR, "")
        };
        let html = land_picker("", "tok", &d, Some(&unread), &live);
        assert!(
            html.contains("Your LAND · 1 area · 2 parcels</summary>"),
            "{html}"
        );
        assert!(
            html.contains("could not be read from peer.decentraland.org"),
            "{html}"
        );
        assert!(
            !html.contains(" empty \u{b7}") && !html.contains("empty parcels"),
            "unknown contents never count as empty: {html}"
        );
    }
}
