//! Runs: maximal same-key stretches of samples, and transient absorption (PLAN §3.5 steps 2–6).

use std::collections::BTreeMap;

use crate::activity::Member;
use crate::attribute::{Match, Projects};
use crate::config::Config;
use crate::context::{Category, Context};
use crate::presence::Presence;
use crate::sample::Sample;
use crate::segment::{
    sorted_details, Evidence, Interruption, MemberTime, Segment, SegmentKind, AWAY_KEY,
    PASSTHROUGH_KEY_PREFIX,
};

/// A maximal stretch of one effective key, possibly grown by absorption.
pub(crate) struct Run {
    key: String,
    label: String,
    category: Option<Category>,
    away: bool,
    /// A passthrough app (`Config::passthrough_bundle_ids`): always transient.
    is_passthrough: bool,
    /// Code runs: the project they are (a known editor project).
    code_project: Option<String>,
    /// Time of this run's own samples (not absorbed ones).
    own_ms: i64,
    /// Direct project matches among own samples: project → (ms, how it matched).
    matches: BTreeMap<String, (i64, String)>,
    /// Activity runs: member (key, label, category, ms), in order of first appearance.
    members: Vec<(String, String, Category, i64)>,
    start_ms: i64,
    end_ms: i64,
    active_ms: i64,
    passive_ms: i64,
    /// Part of `passive_ms` while media held the display awake.
    media_ms: i64,
    details: BTreeMap<String, i64>,
    interruptions: Vec<Interruption>,
    absorbed: Vec<Evidence>,
    locked: bool,
    /// Longest input idleness seen on an unlocked away sample.
    max_idle_s: f64,
}

impl Run {
    /// `ctx` is `None` for away samples.
    fn new(
        ctx: Option<Context>,
        is_passthrough: bool,
        code_project: Option<String>,
        start_ms: i64,
    ) -> Run {
        let away = ctx.is_none();
        let (key, label, category) = match ctx {
            Some(c) => (c.key, c.label, Some(c.category)),
            None => (AWAY_KEY.to_string(), AWAY_KEY.to_string(), None),
        };
        Run {
            key,
            label,
            category,
            away,
            is_passthrough: is_passthrough && !away,
            code_project,
            own_ms: 0,
            matches: BTreeMap::new(),
            members: Vec::new(),
            start_ms,
            end_ms: start_ms,
            active_ms: 0,
            passive_ms: 0,
            media_ms: 0,
            details: BTreeMap::new(),
            interruptions: Vec::new(),
            absorbed: Vec::new(),
            locked: false,
            max_idle_s: 0.0,
        }
    }

    fn ms(&self) -> i64 {
        self.end_ms - self.start_ms
    }

    /// Absorbed into a neighbour: short runs, and pass-through runs shorter than a glance
    /// (§21.1 — a longer look at timewent or a launcher is credited to nobody).
    fn is_transient(&self, config: &Config) -> bool {
        !self.away
            && (self.ms() < config.transient_ms()
                || (self.is_passthrough && self.ms() < config.glance_ms()))
    }

    /// Only a real (non-away, non-pass-through), non-transient run may take in transients.
    fn can_host(&self, config: &Config) -> bool {
        !self.away && !self.is_passthrough && !self.is_transient(config)
    }

    fn push(
        &mut self,
        sample: &Sample,
        presence: Presence,
        detail: Option<String>,
        weight_ms: i64,
    ) {
        self.end_ms = sample.ts_ms + weight_ms;
        self.own_ms += weight_ms;
        match presence {
            Presence::Active => self.active_ms += weight_ms,
            Presence::Passive => {
                self.passive_ms += weight_ms;
                if sample.media_active {
                    self.media_ms += weight_ms;
                }
            }
            Presence::Away if sample.locked => self.locked = true,
            Presence::Away => self.max_idle_s = self.max_idle_s.max(sample.idle.min_s()),
        }
        if let Some(detail) = detail {
            *self.details.entry(detail).or_insert(0) += weight_ms;
        }
    }

    /// Takes in a transient neighbour: its time counts here, its identity becomes an
    /// interruption.
    fn absorb(&mut self, t: Run, config: &Config) {
        let ms = t.ms();
        self.start_ms = self.start_ms.min(t.start_ms);
        self.end_ms = self.end_ms.max(t.end_ms);
        self.active_ms += t.active_ms;
        self.passive_ms += t.passive_ms;
        self.media_ms += t.media_ms;
        self.absorbed.push(if t.is_passthrough {
            Evidence::AbsorbedPassthrough {
                label: t.label.clone(),
                ms,
            }
        } else {
            Evidence::AbsorbedTransient {
                label: t.label.clone(),
                ms,
                threshold_s: i64::from(config.transient_max_s),
            }
        });
        self.interruptions.push(Interruption {
            key: t.key,
            label: t.label,
            start_ms: t.start_ms,
            ms,
        });
    }

    fn add_member(&mut self, key: String, label: String, category: Category, ms: i64) {
        match self.members.iter_mut().find(|m| m.0 == key) {
            Some(m) => m.3 += ms,
            None => self.members.push((key, label, category, ms)),
        }
    }

    /// Continues with a later run of the same key.
    fn extend(&mut self, next: Run) {
        self.end_ms = next.end_ms;
        self.active_ms += next.active_ms;
        self.passive_ms += next.passive_ms;
        self.media_ms += next.media_ms;
        for (detail, ms) in next.details {
            *self.details.entry(detail).or_insert(0) += ms;
        }
        self.interruptions.extend(next.interruptions);
        self.absorbed.extend(next.absorbed);
        self.own_ms += next.own_ms;
        for (project, (ms, via)) in next.matches {
            self.matches.entry(project).or_insert((0, via)).0 += ms;
        }
        for (key, label, category, ms) in next.members {
            self.add_member(key, label, category, ms);
        }
        self.locked |= next.locked;
        self.max_idle_s = self.max_idle_s.max(next.max_idle_s);
    }

    pub(crate) fn finish(self, config: &Config) -> Segment {
        let ms = self.ms();
        let kind = if self.away {
            SegmentKind::Away
        } else if self.is_passthrough || ms < config.glance_ms() {
            // Includes a transient that had no host to join (a block of only short runs).
            SegmentKind::Glance
        } else {
            SegmentKind::Focus
        };

        let mut evidence = self.absorbed;
        let away_threshold_s = i64::from(config.away_after_s);
        if self.away && self.max_idle_s >= f64::from(config.away_after_s) {
            evidence.push(Evidence::IdleAway {
                idle_run_s: self.max_idle_s.floor() as i64,
                threshold_s: away_threshold_s,
            });
        }
        if self.locked {
            evidence.push(Evidence::Locked);
        }
        if self.is_passthrough && ms >= config.glance_ms() {
            evidence.push(Evidence::PassthroughNotCredited {
                label: self.label.clone(),
                ms,
            });
        } else if self.is_passthrough {
            evidence.push(Evidence::PassthroughGlance { ms });
        } else if kind == SegmentKind::Glance {
            evidence.push(Evidence::Glance {
                ms,
                threshold_s: i64::from(config.glance_max_s),
            });
        }
        if self.passive_ms > 0 {
            evidence.push(Evidence::PassiveTime {
                ms: self.passive_ms,
                threshold_s: i64::from(config.passive_after_s),
            });
        }
        if self.media_ms > 0 {
            evidence.push(Evidence::MediaPassive { ms: self.media_ms });
        }

        let details = sorted_details(self.details);
        let mut members = self.members;
        members.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.1.cmp(&b.1)));
        let mut category = self.category;
        if let Some(largest) = members.first() {
            category = Some(largest.2);
            let activity = self.label.clone();
            evidence.splice(
                0..0,
                members.iter().map(|m| Evidence::UserActivity {
                    activity: activity.clone(),
                    member: m.1.clone(),
                }),
            );
        }
        let members: Vec<MemberTime> = members
            .into_iter()
            .map(|(key, label, category, ms)| MemberTime {
                key,
                label,
                ms,
                category: Some(category),
            })
            .collect();
        let project = if !config.attribute_projects || self.away || self.is_passthrough {
            None
        } else if self.code_project.is_some() {
            self.code_project
        } else {
            // Most matched time, then name; it must cover at least half of the run's own time.
            let best = self
                .matches
                .into_iter()
                .max_by(|a, b| a.1 .0.cmp(&b.1 .0).then_with(|| b.0.cmp(&a.0)));
            match best {
                Some((project, (ms, via))) if ms * 2 >= self.own_ms => {
                    evidence.push(Evidence::ProjectMatch {
                        project: project.clone(),
                        via,
                    });
                    Some(project)
                }
                _ => None,
            }
        };

        Segment {
            start_ms: self.start_ms,
            end_ms: self.end_ms,
            key: self.key,
            label: self.label,
            category,
            kind,
            active_ms: self.active_ms,
            passive_ms: self.passive_ms,
            details,
            interruptions: self.interruptions,
            evidence,
            project,
            members,
            listening: Vec::new(),
        }
    }
}

/// Run-length groups a gap-free block by effective key (`"away"` or the context key).
/// `contexts[i]` / `matches[i]` are that sample's context and direct project match,
/// computed once by the caller.
pub(crate) fn group_runs(
    samples: &[Sample],
    contexts: &[Context],
    members: &[Option<Member>],
    matches: &[Match],
    presence: &[Presence],
    projects: &Projects,
    config: &Config,
) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for (i, (sample, &p)) in samples.iter().zip(presence).enumerate() {
        let weight_ms = match samples.get(i + 1) {
            Some(next) => (next.ts_ms - sample.ts_ms).max(0),
            None => config.poll_ms(),
        };
        let is_passthrough = config.is_passthrough(&sample.bundle_id);
        let mut ctx = (p != Presence::Away).then(|| {
            let mut c = contexts[i].clone();
            if is_passthrough {
                c.key = format!("{PASSTHROUGH_KEY_PREFIX}{}", sample.bundle_id);
            }
            c
        });
        let detail = ctx.as_mut().and_then(|c| c.detail.take());
        let key = ctx.as_ref().map_or(AWAY_KEY, |c| c.key.as_str());
        let code_project = ctx.as_ref().and_then(|c| projects.of_code(c));
        let direct = (p != Presence::Away && !is_passthrough && code_project.is_none())
            .then(|| matches[i].clone())
            .flatten();
        if runs.last().is_none_or(|run| run.key != key) {
            runs.push(Run::new(ctx, is_passthrough, code_project, sample.ts_ms));
        }
        if let Some(run) = runs.last_mut() {
            run.push(sample, p, detail, weight_ms);
            if let Some((project, via)) = direct {
                run.matches.entry(project).or_insert((0, via)).0 += weight_ms;
            }
            if let (Some(m), true) = (&members[i], p != Presence::Away) {
                run.add_member(m.key.clone(), m.label.clone(), m.category, weight_ms);
            }
        }
    }
    runs
}

/// Single left-to-right pass over a gap-free block (PLAN §3.5 step 4).
///
/// A transient joins an adjacent host: the previous run, else the next one. Hosts are never
/// away runs, so absorbed activity can never be counted as away time. With no host on
/// either side, transients stay as their own (glance) segments, in order.
pub(crate) fn absorb_transients(runs: Vec<Run>, config: &Config) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    // Transients with no host before them; they join the next run if it can host.
    let mut pending: Vec<Run> = Vec::new();
    let mut runs = runs.into_iter().peekable();
    while let Some(run) = runs.next() {
        if run.is_transient(config) {
            match out.last_mut().filter(|prev| prev.can_host(config)) {
                Some(prev) => {
                    let bridges_same_key = runs.peek().is_some_and(|next| next.key == prev.key);
                    prev.absorb(run, config);
                    if bridges_same_key {
                        if let Some(next) = runs.next() {
                            prev.extend(next);
                        }
                    }
                }
                None => pending.push(run),
            }
        } else if run.can_host(config) {
            let mut host = run;
            for t in pending.drain(..) {
                host.absorb(t, config);
            }
            out.push(host);
        } else {
            // An away run: pending transients had no host on either side.
            out.append(&mut pending);
            out.push(run);
        }
    }
    out.append(&mut pending);
    out
}
