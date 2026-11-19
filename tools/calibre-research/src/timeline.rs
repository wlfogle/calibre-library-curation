//! Universe-wide and independent sub-series ranks derived from fictional time.
//! Publication/edition/author-recommended numbers are retained as labels, never edges.

use anyhow::{bail, Context, Result};
use curator_core::{ai::Citation, matching::normalize, model::Source};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Membership {
    pub name: String,
    pub parent: Option<String>,
    pub published_number: Option<f64>,
    pub citations: Vec<Citation>,
}

/// One book's position at one level; child series retain independent indices.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeriesHierarchy {
    pub name: String,
    pub index: Option<f64>,
    pub subseries: Vec<SeriesHierarchy>,
}

impl SeriesHierarchy {
    pub fn from_memberships(
        universe: &str,
        index: Option<f64>,
        memberships: &[Membership],
        ranks: &[(String, Option<f64>)],
    ) -> Result<Self> {
        let root = normalize(universe);
        if root.is_empty() || index.is_some_and(|n| !n.is_finite()) {
            bail!("Invalid universe name or index");
        }
        let mut members = BTreeMap::new();
        let mut parents = BTreeMap::new();
        let mut positions = BTreeMap::new();
        for (name, rank) in ranks {
            if rank.is_some_and(|n| !n.is_finite()) {
                bail!("Invalid sub-series index");
            }
            if let Some(previous) = positions.insert(normalize(name), *rank) {
                if previous != *rank {
                    bail!("Conflicting sub-series indices");
                }
            }
        }
        for member in memberships {
            let id = normalize(&member.name);
            let parent = member
                .parent
                .as_ref()
                .map(|p| normalize(p))
                .filter(|p| p != &root);
            if id.is_empty() || id == root || parent.as_ref().is_some_and(String::is_empty) {
                bail!("Invalid sub-series name or parent");
            }
            if let Some(previous) = parents.insert(id.clone(), parent.clone()) {
                if previous != parent {
                    bail!("Conflicting sub-series parents");
                }
            }
            members.insert(id, member);
        }
        // Also detect cycles in branches disconnected from the universe root.
        for id in members.keys() {
            let mut path = BTreeSet::new();
            let mut current = Some(id);
            while let Some(next) = current {
                if !path.insert(next) {
                    bail!("Cyclic sub-series hierarchy");
                }
                current = parents
                    .get(next)
                    .context("Unknown parent sub-series")?
                    .as_ref();
            }
        }
        fn children(
            parent: Option<&str>,
            members: &BTreeMap<String, &Membership>,
            parents: &BTreeMap<String, Option<String>>,
            ranks: &BTreeMap<String, Option<f64>>,
        ) -> Vec<SeriesHierarchy> {
            members
                .iter()
                .filter(|(id, _)| parents[*id].as_deref() == parent)
                .map(|(id, member)| SeriesHierarchy {
                    name: member.name.clone(),
                    index: ranks.get(id).copied().flatten(),
                    subseries: children(Some(id), members, parents, ranks),
                })
                .collect()
        }
        Ok(Self {
            name: universe.into(),
            index,
            subseries: children(None, &members, &parents, &positions),
        })
    }
}

fn words(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '#' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn labelled_series_number(text: &str, series: &str, expected: f64) -> Result<bool> {
    let name = words(series);
    if name.is_empty() {
        return Ok(false);
    }
    let name = regex::escape(name.strip_prefix("the ").unwrap_or(&name));
    let value = r"(\d+(?:\.\d+)?|first|second|third|fourth|fifth|sixth|seventh|eighth|ninth|tenth|eleventh|twelfth|thirteenth|fourteenth|fifteenth|sixteenth|seventeenth|eighteenth|nineteenth|twentieth)";
    let ordinal = [
        "first",
        "second",
        "third",
        "fourth",
        "fifth",
        "sixth",
        "seventh",
        "eighth",
        "ninth",
        "tenth",
        "eleventh",
        "twelfth",
        "thirteenth",
        "fourteenth",
        "fifteenth",
        "sixteenth",
        "seventeenth",
        "eighteenth",
        "nineteenth",
        "twentieth",
    ];
    let text = words(text);
    for pattern in [
        format!(r"(?:the )?{name}(?: series)? (?:book|volume|vol|#) ?{value}\b"),
        format!(r"(?:book|volume|vol) ?{value} (?:in|of) (?:the )?{name}\b"),
        format!(r"{value} (?:book|novel|volume) (?:in|of) (?:the )?{name}\b"),
    ] {
        let re = Regex::new(&pattern)?;
        for capture in re.captures_iter(&text) {
            let token = &capture[1];
            let actual = token.parse::<f64>().ok().or_else(|| {
                ordinal
                    .iter()
                    .position(|word| *word == token)
                    .map(|i| (i + 1) as f64)
            });
            if actual == Some(expected) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn date_bound_to_story(setting: &Setting) -> bool {
    let literal = setting.literal.to_lowercase();
    setting.citations.iter().any(|citation| {
        let quote = citation.quote.to_lowercase();
        quote.match_indices(&literal).any(|(position, _)| {
            let prefix = &quote[..position];
            let publication = [
                "published",
                "publication",
                "copyright",
                "release date",
                "edition date",
            ]
            .iter()
            .filter_map(|s| prefix.rfind(s))
            .max();
            let story = [
                "set in",
                "takes place",
                "time of work",
                "story date",
                "fictional date",
                "in-universe chronology",
                "in universe chronology",
                "principal narrative",
            ]
            .iter()
            .filter_map(|s| prefix.rfind(s))
            .max();
            publication.is_none() || story.is_some_and(|s| s > publication.unwrap())
        })
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setting {
    pub calendar: String,
    pub literal: String,
    pub role: String,
    pub citations: Vec<Citation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    pub title: String,
    pub authors: Vec<String>,
    pub universe: Option<String>,
    pub standalone: bool,
    pub memberships: Vec<Membership>,
    pub settings: Vec<Setting>,
    pub contains: Vec<String>,
    pub citations: Vec<Citation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub universe: String,
    pub before: String,
    pub after: String,
    /// Story-time constraints order books; story_time_list is derived only by Rust.
    pub basis: String,
    pub citations: Vec<Citation>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    pub works: Vec<Work>,
    pub relations: Vec<Relation>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evidence {
    pub source: Source,
    pub facts: Facts,
    pub fetched_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub first: i64,
    pub last: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Placement {
    pub work_key: String,
    pub title: String,
    pub universe: String,
    pub global_rank: Option<f64>,
    pub memberships: Vec<(String, Option<f64>)>,
    pub hierarchy: SeriesHierarchy,
    pub placement_policy: String,
    pub problems: Vec<String>,
    pub source_ids: Vec<String>,
}

pub fn key(work: &Work) -> String {
    let mut authors = work
        .authors
        .iter()
        .map(|s| normalize(s))
        .collect::<Vec<_>>();
    authors.sort();
    format!("{}|{}", normalize(&work.title), authors.join("|"))
}

fn proof(citations: &[Citation], source: &Source) -> Result<String> {
    curator_core::ai::citations_text(citations, &source.segments)
}

pub(crate) fn author_cited(text: &str, author: &str) -> bool {
    let letters = author
        .chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| regex::escape(&c.to_string()))
        .collect::<Vec<_>>();
    !letters.is_empty()
        && Regex::new(&format!(r"(?i)\b{}\b", letters.join(r"[\s\p{P}]*")))
            .is_ok_and(|pattern| pattern.is_match(text))
}

pub fn interval(calendar: &str, literal: &str) -> Result<Interval> {
    let raw = literal.trim().replace(['–', '—'], "-");
    let seasonal = Regex::new(concat!(
        r"(?i)^\s*(-?\d+)\s*(DR|AD|CE|PC|AC)",
        r"(?:\s*,?\s*(?:SPR(?:ING)?|SUM(?:MER)?|AUT(?:UMN)?|FALL?|WIN(?:TER)?))?",
        r"\s*-\s*(-?\d+)\s*(DR|AD|CE|PC|AC)",
        r"(?:\s*,?\s*(?:SPR(?:ING)?|SUM(?:MER)?|AUT(?:UMN)?|FALL?|WIN(?:TER)?))?\s*$"
    ))?;
    if let Some(caps) = seasonal.captures(&raw) {
        let a = caps[1].parse::<i64>()?;
        let b = caps[3].parse::<i64>()?;
        let suffix_a = caps[2].to_uppercase();
        let suffix_b = caps[4].to_uppercase();
        let compatible = |suffix: &str| match calendar.to_lowercase().as_str() {
            "dr" => suffix == "DR",
            "krynn" => ["PC", "AC"].contains(&suffix),
            "ad" | "ce" => ["AD", "CE"].contains(&suffix),
            _ => false,
        };
        if !compatible(&suffix_a) || !compatible(&suffix_b) {
            bail!("Date calendar does not match its cited era");
        }
        let first = if suffix_a == "PC" {
            a.checked_neg().context("Date overflow")?
        } else {
            a
        };
        let last = if suffix_b == "PC" {
            b.checked_neg().context("Date overflow")?
        } else {
            b
        };
        if first > last {
            bail!("Reversed date range");
        }
        return Ok(Interval { first, last });
    }
    let imperial = Regex::new(
        r"(?i)^\s*(?:~\s*|c\.?\s*)?(?:\d\.)?(?:\d{3}\.)?(?:(\d{1,3})\s*-\s*)?(\d{1,3})(s)?\.M(\d{1,2})\s*\??\s*$",
    )?;
    if calendar.eq_ignore_ascii_case("imperial") {
        let caps = imperial
            .captures(&raw)
            .context("No explicit Imperial date")?;
        let millennium = caps[4].parse::<i64>()?;
        if millennium == 0 {
            bail!("Invalid millennium");
        }
        let base = (millennium - 1) * 1000;
        let end = caps[2].parse::<i64>()?;
        let start = caps
            .get(1)
            .map(|c| c.as_str().parse::<i64>())
            .transpose()?
            .unwrap_or(end);
        let last = if caps.get(3).is_some() { end + 9 } else { end };
        if start > last || last > 999 {
            bail!("Invalid Imperial range");
        }
        return Ok(Interval {
            first: base + start,
            last: base + last,
        });
    }
    let years = Regex::new(
        r"(?i)^\s*(?:c(?:irca)?\.?\s*|~\s*)?(-?\d+)(?:\s*-\s*(-?\d+))?\s*(PC|AC|DR|AD|CE)?\s*\??\s*$",
    )?;
    let caps = years
        .captures(&raw)
        .context("Date is not an explicit year/range")?;
    let a = caps[1].parse::<i64>()?;
    let mut b = caps
        .get(2)
        .map(|c| c.as_str().parse::<i64>())
        .transpose()?
        .unwrap_or(a);
    let suffix = caps
        .get(3)
        .map(|c| c.as_str().to_uppercase())
        .unwrap_or_default();
    if suffix != "PC" && a >= 0 && b >= 0 && b < a {
        if let Some(end) = caps.get(2).filter(|c| c.as_str().len() < caps[1].len()) {
            let scale = 10_i64
                .checked_pow(end.as_str().len() as u32)
                .context("Abbreviated date range overflows")?;
            b = (a / scale)
                .checked_mul(scale)
                .and_then(|base| base.checked_add(b))
                .context("Abbreviated date range overflows")?;
            if b < a {
                b = b
                    .checked_add(scale)
                    .context("Abbreviated date range overflows")?;
            }
        }
    }
    let compatible = match calendar.to_lowercase().as_str() {
        "krynn" => ["PC", "AC"].contains(&suffix.as_str()),
        "dr" => suffix == "DR",
        "ad" | "ce" => ["", "AD", "CE"].contains(&suffix.as_str()),
        _ => false,
    };
    if !compatible {
        bail!("Date calendar does not match its cited era");
    }
    if !["krynn", "dr", "ad", "ce"].contains(&calendar.to_lowercase().as_str()) {
        bail!("Calendar conversion is not established; do not compare unrelated dates");
    }
    let (first, last) = if suffix == "PC" {
        (-a.max(b), -a.min(b))
    } else {
        (a, b)
    };
    if first > last {
        bail!("Reversed date range");
    }
    Ok(Interval { first, last })
}

fn temporal_language(text: &str) -> bool {
    [
        "chronolog",
        "in-universe",
        "in universe",
        "set in",
        "set before",
        "set after",
        "takes place",
        "time of work",
        "story time",
        "story date",
        "fictional date",
        "fictional",
        "principal narrative",
    ]
    .iter()
    .any(|s| text.to_lowercase().contains(s))
}

pub fn validate(evidence: &Evidence) -> Result<()> {
    if evidence.facts.works.len() > 12 || evidence.facts.relations.len() > 32 {
        bail!("Fact response exceeds limits");
    }
    for work in &evidence.facts.works {
        let text = proof(&work.citations, &evidence.source)?;
        let normalized = normalize(&text);
        if work.title.trim().is_empty()
            || !normalized.contains(&normalize(&work.title))
            || work.authors.is_empty()
            || !work.authors.iter().any(|a| author_cited(&text, a))
        {
            bail!("Work title/author is not established by cited source text");
        }
        if let Some(universe) = &work.universe {
            if !normalized.contains(&normalize(universe)) {
                bail!("Universe membership is not cited");
            }
        }
        if work.standalone {
            if !work.memberships.is_empty()
                || work.universe.is_some()
                || !["standalone", "stand-alone", "non-series", "non series"]
                    .iter()
                    .any(|s| text.to_lowercase().contains(s))
            {
                bail!("Standalone status requires an explicit source, not an empty library tag");
            }
        }
        for membership in &work.memberships {
            let text = proof(&membership.citations, &evidence.source)?;
            if !normalize(&text).contains(&normalize(&membership.name)) {
                bail!("Series membership lacks a literal citation");
            }
            if let Some(parent) = &membership.parent {
                if !normalize(&text).contains(&normalize(parent)) {
                    bail!("Parent-series relationship lacks a citation");
                }
            }
            if membership
                .published_number
                .is_some_and(|n| !n.is_finite() || n <= 0.0)
            {
                bail!("Invalid published series label");
            }
            if let Some(number) = membership.published_number {
                if !normalize(&text).contains(&normalize(&work.title))
                    || !labelled_series_number(&text, &membership.name, number)?
                {
                    bail!(
                        "Series label number is not tied to this work and series in the citation"
                    );
                }
            }
        }
        for setting in &work.settings {
            let text = proof(&setting.citations, &evidence.source)?;
            if !normalize(&text).contains(&normalize(&work.title))
                || !normalize(&text).contains(&normalize(&setting.literal))
                || !temporal_language(&text)
                || !date_bound_to_story(setting)
                || !["principal", "prologue", "flashback", "visited", "alternate"]
                    .contains(&setting.role.as_str())
            {
                bail!("Fictional setting/date is not established");
            }
            let claimed = interval(&setting.calendar, &setting.literal)?;
            for citation in &setting.citations {
                if let Some(row) = evidence
                    .source
                    .segments
                    .iter()
                    .find(|s| s.ordinal == citation.segment && s.kind == "tr")
                {
                    for cell in row.text.split(" | ") {
                        let value = cell
                            .split_once(':')
                            .map(|(_, v)| v.trim())
                            .unwrap_or(cell.trim());
                        if normalize(value).contains(&normalize(&setting.literal)) {
                            if let Ok(actual) = interval(&setting.calendar, value) {
                                if actual != claimed {
                                    bail!("Date claim truncates the cited story interval");
                                }
                            }
                        }
                    }
                }
            }
        }
        for part in &work.contains {
            if !normalized.contains(&normalize(part))
                || ![
                    "contains",
                    "includes",
                    "brings together",
                    "collection",
                    "omnibus",
                ]
                .iter()
                .any(|s| text.to_lowercase().contains(s))
            {
                bail!("Collection contents are not established");
            }
        }
    }
    for relation in &evidence.facts.relations {
        let text = proof(&relation.citations, &evidence.source)?;
        let normalized = normalize(&text);
        if !normalized.contains(&normalize(&relation.universe))
            || !normalized.contains(&normalize(&relation.before))
            || !normalized.contains(&normalize(&relation.after))
        {
            bail!("Ordering relationship omits its works or universe");
        }
        if ![
            "story_time",
            "story_time_list",
            "publication",
            "recommended",
        ]
        .contains(&relation.basis.as_str())
        {
            bail!("Unknown ordering basis");
        }
        if relation.basis == "publication"
            && !["publication", "published", "release order"]
                .iter()
                .any(|s| text.to_lowercase().contains(s))
        {
            bail!("Publication relationship is not explicitly sourced");
        }
        if relation.basis == "recommended"
            && !["recommend", "reading order"]
                .iter()
                .any(|s| text.to_lowercase().contains(s))
        {
            bail!("Recommended relationship is not explicitly sourced");
        }
        if relation.basis == "story_time_list" && !ordered_direction(relation, &evidence.source) {
            bail!("Chronology-list relationship is not established by the source structure");
        }
        if relation.basis == "story_time" {
            if !temporal_language(&text) {
                bail!("A sequel/publication label alone is not fictional-time ordering");
            }
            let before = normalize(&relation.before);
            let after = normalize(&relation.after);
            let forward = ["", "is", "isset", "takesplace", "occurs", "happens"]
                .iter()
                .any(|verb| {
                    normalized.contains(&format!("{before}{verb}before{after}"))
                        || normalized.contains(&format!("{after}{verb}after{before}"))
                });
            let uncertain =
                Regex::new(r"(?i)\b(?:not|never|uncertain|unconfirmed|disputed|might|possibly)\b")?
                    .is_match(&text);
            if !forward || uncertain {
                bail!("Citations do not establish the claimed temporal direction");
            }
        }
    }
    Ok(())
}

fn ordered_coordinates(segment: &curator_core::model::Segment) -> Option<(usize, usize, usize)> {
    let parts = segment.kind.split(':').collect::<Vec<_>>();
    if parts.len() != 4 || parts[0] != "ordered" {
        return None;
    }
    let values = (
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
        parts[3].parse().ok()?,
    );
    if values.0 == 0 || values.1 == 0 || values.2 == 0 {
        return None;
    }
    Some(values)
}

pub(crate) fn chronological_heading(text: &str, universe: &str) -> bool {
    let text = text.to_lowercase();
    normalize(&text).contains(&normalize(universe))
        && text.contains("chronolog")
        && ![
            "publication",
            "published",
            "release",
            "recommend",
            "possible",
            "uncertain",
            "disputed",
        ]
        .iter()
        .any(|s| text.contains(s))
}

fn entry_names(segment: &curator_core::model::Segment, title: &str) -> bool {
    let Some((_, number, _)) = ordered_coordinates(segment) else {
        return false;
    };
    let prefix = format!("{number}. ");
    let Some(label) = segment.text.strip_prefix(&prefix) else {
        return false;
    };
    let label = label.to_lowercase();
    let title = title.trim().to_lowercase();
    if title.is_empty() {
        return false;
    }
    let Some(suffix) = label.strip_prefix(&title) else {
        return false;
    };
    let suffix = suffix.trim_start();
    suffix.is_empty()
        || ["(", "[", "—", "–", "-", "by "]
            .iter()
            .any(|s| suffix.starts_with(s))
}

fn ordered_direction(relation: &Relation, source: &Source) -> bool {
    let cited = |s: &curator_core::model::Segment| {
        relation
            .citations
            .iter()
            .any(|c| c.segment == s.ordinal && c.quote == s.text)
    };
    let before = source
        .segments
        .iter()
        .filter(|s| cited(s) && entry_names(s, &relation.before))
        .collect::<Vec<_>>();
    let after = source
        .segments
        .iter()
        .filter(|s| cited(s) && entry_names(s, &relation.after))
        .collect::<Vec<_>>();
    if before.len() != 1 || after.len() != 1 {
        return false;
    }
    let a = before[0];
    let b = after[0];
    let (Some((heading, first, size)), Some((other, last, other_size))) =
        (ordered_coordinates(a), ordered_coordinates(b))
    else {
        return false;
    };
    if heading != other || size != other_size || first >= last || a.ordinal >= b.ordinal {
        return false;
    }
    let mut positions = BTreeSet::new();
    if source
        .segments
        .iter()
        .filter_map(ordered_coordinates)
        .filter(|(h, _, s)| *h == heading && *s == size)
        .any(|(_, n, _)| !positions.insert(n))
    {
        return false;
    }
    source.segments.iter().any(|s| {
        s.ordinal == heading
            && s.kind.starts_with('h')
            && cited(s)
            && chronological_heading(&s.text, &relation.universe)
    })
}

/// HTML list positions are evidence of relationships, never final Calibre ranks.
pub fn ordered_relationships(source: &Source, works: &[Work]) -> Vec<Relation> {
    let mut groups: BTreeMap<(usize, usize), Vec<&curator_core::model::Segment>> = BTreeMap::new();
    for segment in &source.segments {
        if let Some((heading, _, size)) = ordered_coordinates(segment) {
            groups.entry((heading, size)).or_default().push(segment);
        }
    }
    let mut output = Vec::new();
    for ((heading, _), mut rows) in groups {
        let Some(context) = source.segments.iter().find(|s| s.ordinal == heading) else {
            continue;
        };
        rows.sort_by_key(|s| s.ordinal);
        for pair in rows.windows(2) {
            let identify = |row| {
                let matches = works
                    .iter()
                    .filter(|w| entry_names(row, &w.title))
                    .map(|w| ((key(w), w.universe.clone()), w))
                    .collect::<BTreeMap<_, _>>();
                if matches.len() == 1 {
                    matches.values().next().copied()
                } else {
                    None
                }
            };
            let (Some(a), Some(b)) = (identify(pair[0]), identify(pair[1])) else {
                continue;
            };
            let Some(universe) = a
                .universe
                .as_ref()
                .filter(|u| b.universe.as_ref() == Some(u))
            else {
                continue;
            };
            if !chronological_heading(&context.text, universe) {
                continue;
            }
            let relation = Relation {
                universe: universe.clone(),
                before: a.title.clone(),
                after: b.title.clone(),
                basis: "story_time_list".into(),
                citations: [context, pair[0], pair[1]]
                    .iter()
                    .map(|s| Citation {
                        segment: s.ordinal,
                        quote: s.text.clone(),
                    })
                    .collect(),
            };
            if ordered_direction(&relation, source) {
                output.push(relation);
            }
        }
    }
    output
}

#[derive(Default)]
struct Node {
    work: Option<Work>,
    memberships: BTreeMap<String, Membership>,
    settings: Vec<Setting>,
    sources: BTreeSet<String>,
    problems: Vec<String>,
}

/// Derive order for all known works, including unowned works present in evidence.
pub fn solve(evidence: &[Evidence]) -> Result<Vec<Placement>> {
    let mut nodes: BTreeMap<String, Node> = BTreeMap::new();
    for bundle in evidence {
        validate(bundle)?;
        for work in &bundle.facts.works {
            let node = nodes.entry(key(work)).or_default();
            if let Some(old) = &node.work {
                if old.universe != work.universe
                    && old.universe.is_some()
                    && work.universe.is_some()
                {
                    node.problems
                        .push("Conflicting universe memberships".into());
                }
                if old.standalone != work.standalone
                    && (old.universe.is_some() || work.universe.is_some())
                {
                    node.problems
                        .push("Conflicting standalone/series evidence".into());
                }
            }
            if node.work.is_none() || work.universe.is_some() {
                node.work = Some(work.clone());
            }
            for membership in &work.memberships {
                let id = normalize(&membership.name);
                if node.memberships.get(&id).is_some_and(|old| {
                    old.parent != membership.parent
                        || (old.published_number.is_some()
                            && membership.published_number.is_some()
                            && old.published_number != membership.published_number)
                }) {
                    node.problems.push(format!(
                        "Conflicting membership evidence for {}",
                        membership.name
                    ));
                } else {
                    node.memberships.insert(id, membership.clone());
                }
            }
            node.settings.extend(work.settings.clone());
            node.sources.insert(bundle.source.id.clone());
        }
    }
    let mut output = Vec::new();
    let universes: BTreeSet<String> = nodes
        .values()
        .filter_map(|n| n.work.as_ref().and_then(|w| w.universe.clone()))
        .collect();
    for universe in universes {
        let group = nodes
            .iter()
            .filter(|(_, n)| {
                n.work.as_ref().and_then(|w| w.universe.as_deref()) == Some(universe.as_str())
            })
            .collect::<Vec<_>>();
        let ids: BTreeSet<String> = group.iter().map(|(id, _)| (*id).clone()).collect();
        let mut edges: BTreeMap<String, BTreeSet<String>> =
            ids.iter().map(|id| (id.clone(), BTreeSet::new())).collect();
        let mut by_title: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut dates = BTreeMap::new();
        let mut issues = Vec::new();
        for (id, node) in &group {
            let work = node.work.as_ref().context("Missing work")?;
            by_title
                .entry(normalize(&work.title))
                .or_default()
                .push((*id).clone());
            issues.extend(node.problems.clone());
            let principal = node
                .settings
                .iter()
                .filter(|s| s.role == "principal")
                .map(|s| {
                    Ok((
                        s.calendar.to_lowercase(),
                        interval(&s.calendar, &s.literal)?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            if let Some(first) = principal.first() {
                if principal.iter().any(|item| item != first) {
                    issues.push(format!("Conflicting principal dates for {}", work.title));
                } else {
                    dates.insert((*id).clone(), first.clone());
                }
            }
        }
        for (a, (calendar_a, range_a)) in &dates {
            for (b, (calendar_b, range_b)) in &dates {
                if a != b && calendar_a == calendar_b && range_a.last < range_b.first {
                    edges.get_mut(a).unwrap().insert(b.clone());
                }
            }
        }
        let mut temporal_nodes: BTreeSet<String> = dates.keys().cloned().collect();
        for bundle in evidence {
            for relation in &bundle.facts.relations {
                if relation.universe != universe
                    || !["story_time", "story_time_list"].contains(&relation.basis.as_str())
                {
                    continue;
                }
                let before = by_title.get(&normalize(&relation.before));
                let after = by_title.get(&normalize(&relation.after));
                if let (Some(a), Some(b)) = (before, after) {
                    if a.len() == 1 && b.len() == 1 {
                        edges.get_mut(&a[0]).unwrap().insert(b[0].clone());
                        temporal_nodes.insert(a[0].clone());
                        temporal_nodes.insert(b[0].clone());
                    } else {
                        issues.push("Ambiguous work identity in ordering relationship".into());
                    }
                }
            }
        }
        if ids.iter().any(|id| !temporal_nodes.contains(id)) {
            issues.push("Membership alone does not establish fictional-time placement".into());
        }
        // Contents establish coverage, not a claim that a whole collection occurs
        // between its first story and the following standalone work.
        for (_, node) in &group {
            let work = node.work.as_ref().unwrap();
            if !work.contains.is_empty() {
                issues.push(format!(
                    "Collection {} spans constituent works; exact whole-book placement needs temporal evidence",
                    work.title
                ));
            }
        }
        let mut indegree: BTreeMap<String, usize> = ids.iter().map(|id| (id.clone(), 0)).collect();
        for children in edges.values() {
            for child in children {
                *indegree.get_mut(child).unwrap() += 1;
            }
        }
        let mut order: Vec<String> = Vec::new();
        let policy = "fictional dates and explicit story-time relationships; no publication/recommended/alphabetical fallback".to_string();
        while order.len() < ids.len() {
            let candidates = indegree
                .iter()
                .filter(|(id, degree)| **degree == 0 && !order.contains(*id))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            if candidates.is_empty() {
                issues.push("Contradictory/cyclic fictional chronology".into());
                break;
            }
            if candidates.len() > 1 {
                issues.push(
                    "No unique in-universe order is established; research missing temporal relationships"
                        .into(),
                );
                break;
            }
            let chosen = candidates[0].clone();
            order.push(chosen.clone());
            for child in &edges[&chosen] {
                *indegree.get_mut(child).unwrap() -= 1;
            }
        }
        let established = issues.is_empty() && order.len() == ids.len();
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut member_ranks = BTreeMap::new();
        if established {
            for id in &order {
                for name in nodes[id].memberships.keys() {
                    *counts.entry(name.clone()).or_default() += 1;
                    member_ranks.insert((id.clone(), name.clone()), counts[name] as f64);
                }
            }
        }
        for (id, node) in group {
            let work = node.work.as_ref().unwrap();
            let global_rank = if established {
                order.iter().position(|i| i == id).map(|i| (i + 1) as f64)
            } else {
                None
            };
            let memberships = node
                .memberships
                .iter()
                .map(|(name, value)| {
                    (
                        value.name.clone(),
                        member_ranks.get(&(id.clone(), name.clone())).copied(),
                    )
                })
                .collect::<Vec<_>>();
            let hierarchy = SeriesHierarchy::from_memberships(
                &universe,
                global_rank,
                &node.memberships.values().cloned().collect::<Vec<_>>(),
                &memberships,
            )?;
            output.push(Placement {
                work_key: id.clone(),
                title: work.title.clone(),
                universe: universe.clone(),
                global_rank,
                memberships,
                hierarchy,
                placement_policy: policy.clone(),
                problems: issues.clone(),
                source_ids: node.sources.iter().cloned().collect(),
            });
        }
    }
    Ok(output)
}
