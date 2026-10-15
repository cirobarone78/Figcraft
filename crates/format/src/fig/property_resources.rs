//! Bounded provenance for Boolean properties declared on excluded .fig library pages.
//!
//! Original owner/definition identifiers are retained data, not identifiers allocated by the
//! writer. Standard hidden SYMBOL definitions carry the original defaults; the private packet
//! only preserves native names and their scope across GUID allocation and REST renaming.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::kiwi::Kv;
use crate::FormatError;

pub(crate) const VERSION: u8 = 1;
pub(crate) const CATALOG_KEY: &str = "figcraft.figProperties.catalog";
pub(crate) const MAIN_KEY: &str = "figcraft.figProperties.main";
pub(crate) const BINDINGS_KEY: &str = "figcraft.figProperties.bindings";
pub(crate) const PLUGIN_ID: &str = "figcraft";
pub(crate) const PLUGIN_KEY: &str = "property-resources";
pub(crate) const FIG_PACKET_FIELD: &str = "_figPropertyPacket";
pub(crate) const FIG_CATALOGS_FIELD: &str = "_figPropertyCatalogs";

pub(crate) const MAX_CATALOG_BYTES: usize = 1 << 20;
pub(crate) const MAX_PACKET_BYTES: usize = 64 << 10;
pub(crate) const MAX_TOTAL_BYTES: usize = 16 << 20;
/// A wire packet wraps its native packet (or catalog) in an envelope, a definition remap and
/// one alias per active binding, each no larger than the binding it names. The wire caps leave
/// room for that overhead so that any native packet accepted on import can be written again.
pub(crate) const MAX_WIRE_PACKET_BYTES: usize = 4 * MAX_PACKET_BYTES;
pub(crate) const MAX_WIRE_CATALOG_BYTES: usize = 4 * MAX_CATALOG_BYTES;
pub(crate) const MAX_OWNERS: usize = 4096;
pub(crate) const MAX_DEFINITIONS: usize = 8192;
pub(crate) const MAX_BINDINGS: usize = 200_000;
pub(crate) const MAX_NAME_BYTES: usize = 4096;
pub(crate) const MAX_CHAIN: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Id {
    pub session: u32,
    pub local: u32,
}

impl Id {
    pub fn parse(s: &str) -> Option<Self> {
        let (session, local) = s.split_once(':')?;
        if session.is_empty() || local.is_empty() || !session.bytes().chain(local.bytes()).all(|b| b.is_ascii_digit()) {
            return None;
        }
        let id = Self { session: session.parse().ok()?, local: local.parse().ok()? };
        (id.key() == s).then_some(id)
    }

    pub fn key(self) -> String {
        format!("{}:{}", self.session, self.local)
    }

    fn valid(self) -> bool {
        self != (Self { session: 0, local: 0 }) && self != (Self { session: u32::MAX, local: u32::MAX })
    }
}

pub(crate) fn id(v: &Kv<'_>) -> Option<Id> {
    let uint = |key| match v.get(key)? {
        Kv::Uint(n) => u32::try_from(*n).ok(),
        _ => None,
    };
    Some(Id { session: uint("sessionID")?, local: uint("localID")? })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Definition {
    pub id: Id,
    pub name: String,
    pub initial: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Owner {
    pub id: Id,
    pub page: Id,
    pub name: String,
    pub definitions: Vec<Definition>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Catalog {
    pub version: u8,
    pub scope: String,
    pub owners: Vec<Owner>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MainTag {
    pub version: u8,
    pub scope: String,
    pub origin: Id,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub key: String,
    pub definition: Id,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bindings {
    pub version: u8,
    pub scope: String,
    pub main: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_main: Option<Id>,
    pub values: Vec<Binding>,
    pub visible: Option<Binding>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativePacket {
    pub catalog: Option<Catalog>,
    pub main: Option<MainTag>,
    pub bindings: Option<Bindings>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Remap {
    pub original: Id,
    pub wire: Id,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireResources {
    pub scope: String,
    pub page: Id,
    pub owners: Vec<Remap>,
    pub definitions: Vec<Remap>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    Value,
    Visible,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Alias {
    pub role: Role,
    pub key: String,
    pub definition: Id,
    pub wire: Id,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WirePacket {
    pub version: u8,
    pub native: NativePacket,
    pub resources: Option<WireResources>,
    pub aliases: Vec<Alias>,
}

/// Warning kinds for private records that were left out. Each is reported once with a count:
/// the records are provenance only, so they never make a file fail to open or export.
pub(crate) const DROPPED_DAMAGED: &str = "damaged or inconsistent private Boolean property record(s) were left out";
pub(crate) const DROPPED_STALE: &str = "private Boolean property record(s) that no longer match their layer were left out";
pub(crate) const DROPPED_LIMIT: &str = "private Boolean property record(s) over their size limit were left out";
pub(crate) const DROPPED_UNBOUND: &str = "Boolean property value(s) or visibility link(s) without a definition in the file were left out";

/// The warnings for `dropped` (kind → count), one per kind.
pub(crate) fn dropped_warnings(dropped: &BTreeMap<&'static str, usize>) -> Vec<String> {
    dropped.iter().filter(|(_, count)| **count > 0).map(|(kind, count)| format!("{count} {kind}")).collect()
}

pub(crate) fn invalid(reason: &str) -> FormatError {
    FormatError::Corrupt(format!(".fig property resources: {reason}"))
}

pub(crate) fn valid_scope(scope: &str) -> bool {
    scope.len() == 64 && scope.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn name(s: &str) -> bool {
    s.len() <= MAX_NAME_BYTES && !s.contains('\0')
}

fn scope(owners: &[Owner]) -> Result<String, FormatError> {
    let bytes = serialized(&owners, MAX_CATALOG_BYTES)?;
    let hash = Sha256::digest(bytes);
    Ok(hash.iter().map(|b| format!("{b:02x}")).collect())
}

fn check_owners(owners: &[Owner]) -> Result<(), FormatError> {
    if owners.is_empty() || owners.len() > MAX_OWNERS {
        return Err(invalid("catalog owner count is outside its limit"));
    }
    let mut owner_ids = BTreeSet::new();
    let mut rows = 0usize;
    let mut bytes = 0usize;
    let mut names = BTreeMap::new();
    for owner in owners {
        if !owner.id.valid() || !owner.page.valid() || !name(&owner.name) || !owner_ids.insert(owner.id) {
            return Err(invalid("invalid or duplicate original owner"));
        }
        rows = rows.checked_add(owner.definitions.len()).ok_or_else(|| invalid("definition count overflow"))?;
        bytes = bytes.checked_add(owner.name.len()).ok_or_else(|| invalid("catalog byte count overflow"))?;
        if owner.definitions.is_empty() || rows > MAX_DEFINITIONS {
            return Err(invalid("catalog definition count is outside its limit"));
        }
        let mut ids = BTreeSet::new();
        for d in &owner.definitions {
            if !d.id.valid() || d.name.is_empty() || !name(&d.name) || !ids.insert(d.id) {
                return Err(invalid("invalid or duplicate original Boolean definition"));
            }
            bytes = bytes.checked_add(d.name.len()).ok_or_else(|| invalid("catalog byte count overflow"))?;
            if bytes > MAX_CATALOG_BYTES {
                return Err(invalid("catalog exceeds its byte limit"));
            }
            if let Some(previous) = names.insert(d.id, d.name.as_str())
                && previous != d.name.as_str()
            {
                return Err(invalid("shared original definition has inconsistent names"));
            }
        }
    }
    Ok(())
}

pub(crate) fn make_catalog(mut owners: Vec<Owner>) -> Result<Catalog, FormatError> {
    check_owners(&owners)?;
    owners.sort_by_key(|o| o.id);
    for owner in &mut owners {
        owner.definitions.sort_by_key(|d| d.id);
    }
    let scope = scope(&owners)?;
    let catalog = Catalog { version: VERSION, scope, owners };
    validate_catalog(&catalog)?;
    Ok(catalog)
}

pub(crate) fn validate_catalog(c: &Catalog) -> Result<(), FormatError> {
    if c.version != VERSION || !valid_scope(&c.scope) {
        return Err(invalid("unsupported catalog version or scope"));
    }
    check_owners(&c.owners)?;
    if !c.owners.windows(2).all(|w| w[0].id < w[1].id)
        || c.owners.iter().any(|o| !o.definitions.windows(2).all(|w| w[0].id < w[1].id))
        || scope(&c.owners)? != c.scope
    {
        return Err(invalid("catalog is not canonical or its scope does not match"));
    }
    Ok(())
}

fn check_main(m: &MainTag) -> Result<(), FormatError> {
    if m.version != VERSION || !valid_scope(&m.scope) || !m.origin.valid() {
        return Err(invalid("invalid actual-main tag"));
    }
    Ok(())
}

fn check_bindings(b: &Bindings) -> Result<(), FormatError> {
    if b.version != VERSION
        || !valid_scope(&b.scope)
        || !b.main.valid()
        || b.visible_main.is_some_and(|m| !m.valid())
        || b.values.len() + usize::from(b.visible.is_some()) > MAX_BINDINGS
    {
        return Err(invalid("invalid Boolean bindings"));
    }
    let mut keys = BTreeSet::new();
    let mut definitions = BTreeSet::new();
    for binding in &b.values {
        if binding.key.is_empty()
            || !name(&binding.key)
            || !binding.definition.valid()
            || !keys.insert(binding.key.as_str())
            || !definitions.insert(binding.definition)
        {
            return Err(invalid("invalid or duplicate Boolean value binding"));
        }
    }
    if b.visible.as_ref().is_some_and(|b| b.key.is_empty() || !name(&b.key) || !b.definition.valid()) {
        return Err(invalid("invalid Boolean visibility binding"));
    }
    Ok(())
}

fn check_wire(w: &WirePacket) -> Result<(), FormatError> {
    if w.version != VERSION || w.aliases.len() > MAX_BINDINGS {
        return Err(invalid("unsupported resource packet or too many aliases"));
    }
    if let Some(c) = &w.native.catalog {
        validate_catalog(c)?;
    }
    if let Some(m) = &w.native.main {
        check_main(m)?;
    }
    if let Some(b) = &w.native.bindings {
        check_bindings(b)?;
    }
    if let Some(r) = &w.resources {
        let Some(c) = &w.native.catalog else {
            return Err(invalid("resource map has no original catalog"));
        };
        if r.scope != c.scope || !r.page.valid() || r.owners.len() != c.owners.len() || r.definitions.len() > MAX_DEFINITIONS {
            return Err(invalid("invalid catalog wire map"));
        }
        let original_owners: BTreeSet<Id> = c.owners.iter().map(|o| o.id).collect();
        let original_defs: BTreeSet<Id> = c.owners.iter().flat_map(|o| o.definitions.iter().map(|d| d.id)).collect();
        let maps = |remaps: &[Remap], expected: &BTreeSet<Id>| {
            let mut from = BTreeSet::new();
            let mut to = BTreeSet::new();
            remaps.iter().all(|r| r.wire.valid() && from.insert(r.original) && to.insert(r.wire)) && from == *expected
        };
        if !maps(&r.owners, &original_owners) || !maps(&r.definitions, &original_defs) || r.owners.iter().any(|o| o.wire == r.page) {
            return Err(invalid("incomplete or colliding catalog wire map"));
        }
        let mut node_ids: BTreeSet<Id> = r.owners.iter().map(|o| o.wire).collect();
        node_ids.insert(r.page);
        if r.definitions.iter().any(|d| node_ids.contains(&d.wire)) {
            return Err(invalid("definition identifiers collide with resource nodes"));
        }
    } else if w.native.catalog.is_some() {
        return Err(invalid("original catalog has no verifiable wire resources"));
    }
    let mut aliases = BTreeSet::new();
    let values: BTreeMap<&str, Id> =
        w.native.bindings.as_ref().into_iter().flat_map(|b| b.values.iter().map(|v| (v.key.as_str(), v.definition))).collect();
    for a in &w.aliases {
        let Some(bindings) = &w.native.bindings else {
            return Err(invalid("alias has no native binding packet"));
        };
        let definition = match a.role {
            Role::Value => values.get(a.key.as_str()).copied(),
            Role::Visible => bindings.visible.as_ref().filter(|b| b.key == a.key).map(|b| b.definition),
        };
        if definition != Some(a.definition) || !a.wire.valid() || !aliases.insert((a.role, a.key.as_str())) {
            return Err(invalid("invalid or duplicate active Boolean alias"));
        }
    }
    Ok(())
}

fn read<T: serde::de::DeserializeOwned>(s: &str, limit: usize) -> Result<T, FormatError> {
    if s.len() > limit {
        return Err(invalid("resource metadata exceeds its byte limit"));
    }
    serde_json::from_str(s).map_err(|_| invalid("malformed resource metadata"))
}

fn write<T: Serialize>(value: &T, limit: usize) -> Result<String, FormatError> {
    String::from_utf8(serialized(value, limit)?).map_err(|_| invalid("invalid resource serialization"))
}

fn serialized<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, FormatError> {
    struct Capped {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Capped {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.bytes.len().checked_add(bytes.len()).is_none_or(|n| n > self.limit) {
                return Err(io::Error::other("resource byte limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut out = Capped { bytes: Vec::new(), limit };
    serde_json::to_writer(&mut out, value).map_err(|_| invalid("resource metadata exceeds its byte limit"))?;
    Ok(out.bytes)
}

pub(crate) fn read_catalog(s: &str) -> Result<Catalog, FormatError> {
    let c = read(s, MAX_CATALOG_BYTES)?;
    validate_catalog(&c)?;
    Ok(c)
}

pub(crate) fn read_main(s: &str) -> Result<MainTag, FormatError> {
    let m = read(s, MAX_PACKET_BYTES)?;
    check_main(&m)?;
    Ok(m)
}

pub(crate) fn read_bindings(s: &str) -> Result<Bindings, FormatError> {
    let b = read(s, MAX_PACKET_BYTES)?;
    check_bindings(&b)?;
    Ok(b)
}

pub(crate) fn read_wire(s: &str) -> Result<WirePacket, FormatError> {
    let w: WirePacket = read(s, MAX_WIRE_CATALOG_BYTES)?;
    if w.native.catalog.is_none() && s.len() > MAX_WIRE_PACKET_BYTES {
        return Err(invalid("node resource packet exceeds its byte limit"));
    }
    check_wire(&w)?;
    Ok(w)
}

pub(crate) fn write_catalog(c: &Catalog) -> Result<String, FormatError> {
    validate_catalog(c)?;
    write(c, MAX_CATALOG_BYTES)
}

pub(crate) fn write_main(m: &MainTag) -> Result<String, FormatError> {
    check_main(m)?;
    write(m, MAX_PACKET_BYTES)
}

pub(crate) fn write_bindings(b: &Bindings) -> Result<String, FormatError> {
    check_bindings(b)?;
    write(b, MAX_PACKET_BYTES)
}

pub(crate) fn write_wire(w: &WirePacket) -> Result<String, FormatError> {
    check_wire(w)?;
    write(w, if w.native.catalog.is_some() { MAX_WIRE_CATALOG_BYTES } else { MAX_WIRE_PACKET_BYTES })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guid(local: u32) -> Id {
        Id { session: 8, local }
    }
    fn catalog() -> Catalog {
        make_catalog(
            [false, true, true]
                .into_iter()
                .enumerate()
                .map(|(i, initial)| Owner {
                    id: guid(10 + i as u32),
                    page: guid(1),
                    name: format!("Owner {i}"),
                    definitions: vec![Definition { id: guid(2), name: "Flag".into(), initial: Some(initial) }],
                })
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn owners_keep_different_defaults_and_scopes_are_content_bound() {
        let a = catalog();
        assert_eq!(read_catalog(&write_catalog(&a).unwrap()).unwrap(), a);
        let mut owners = a.owners.clone();
        owners[0].definitions[0].initial = Some(true);
        let b = make_catalog(owners).unwrap();
        assert_ne!(a.scope, b.scope);
        let mut forged = a.clone();
        forged.owners[0].definitions[0].initial = Some(true);
        assert!(validate_catalog(&forged).is_err());
    }

    #[test]
    fn catalog_rejects_duplicate_and_ambiguous_definitions() {
        let mut owners = catalog().owners;
        owners.push(owners[0].clone());
        assert!(make_catalog(owners).is_err());
        let mut owners = catalog().owners;
        owners[1].definitions[0].name = "Other".into();
        assert!(make_catalog(owners).is_err());
        let mut owners = catalog().owners;
        let duplicate = owners[0].definitions[0].clone();
        owners[0].definitions.push(duplicate);
        assert!(make_catalog(owners).is_err());
    }

    #[test]
    fn parse_and_serialization_are_capped_and_guid_reader_is_exact() {
        assert!(read_bindings(&" ".repeat(MAX_PACKET_BYTES + 1)).is_err());
        let mut owners = catalog().owners;
        owners[0].name = "x".repeat(MAX_NAME_BYTES + 1);
        assert!(make_catalog(owners).is_err());
        assert_eq!(id(&Kv::Object(vec![("sessionID", Kv::Float(8.0)), ("localID", Kv::Uint(2))])), None);
        assert_eq!(Id::parse("08:2"), None);
        assert_eq!(Id::parse("8:4294967296"), None);
        assert!(serialized(&"\0".repeat(10), 12).is_err(), "escaped bytes must share the serialization budget");
    }

    #[test]
    fn wire_maps_cannot_claim_another_definition_or_resource_node() {
        let c = catalog();
        let scope = c.scope.clone();
        let mut packet = WirePacket {
            version: VERSION,
            native: NativePacket { catalog: Some(c), main: None, bindings: None },
            resources: Some(WireResources {
                scope,
                page: guid(20),
                owners: (0..3).map(|i| Remap { original: guid(10 + i), wire: guid(30 + i) }).collect(),
                definitions: vec![Remap { original: guid(2), wire: guid(40) }],
            }),
            aliases: Vec::new(),
        };
        assert!(write_wire(&packet).is_ok());
        packet.resources.as_mut().unwrap().definitions[0].original = guid(3);
        assert!(write_wire(&packet).is_err());
        packet.resources.as_mut().unwrap().definitions[0] = Remap { original: guid(2), wire: guid(30) };
        assert!(write_wire(&packet).is_err());
    }

    #[test]
    fn value_bindings_cannot_alias_one_definition_twice_but_visibility_can_reference_it() {
        let c = catalog();
        let mut b = Bindings {
            version: VERSION,
            scope: c.scope,
            main: guid(50),
            visible_main: None,
            values: vec![Binding { key: "A".into(), definition: guid(2) }],
            visible: Some(Binding { key: "A".into(), definition: guid(2) }),
        };
        assert!(write_bindings(&b).is_ok());
        b.values.push(Binding { key: "B".into(), definition: guid(2) });
        let malformed = serde_json::to_string(&b).unwrap();
        assert!(read_bindings(&malformed).is_err());
        assert!(write_bindings(&b).is_err());
    }
}
