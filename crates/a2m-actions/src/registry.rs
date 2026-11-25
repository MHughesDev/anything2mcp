//! The registry of actions: the kernel's own, and those providers add.
//! Built once and frozen.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use a2m_core::{CanonicalJson, Digest, EffectClass, Failure};
use a2m_policy::Role;
use serde::Serialize;

use crate::catalog;
use crate::codes;
use crate::def::{
    ActionDef, ActionName, BuiltInToolset, Effect, Faces, Handler, HealthNeed, Input, Listed,
    PersonNeed, RunsIn, Shape, Who,
};
use crate::handlers;

/// The format of the action catalog.
pub const CATALOG_FORMAT: u32 = 1;

/// Every action, by name, in catalog order: the kernel's own first, then
/// each provider's in order of name.
#[derive(Debug, Clone)]
pub struct ActionRegistry {
    defs: Arc<Vec<ActionDef>>,
    by_name: Arc<BTreeMap<ActionName, usize>>,
}

impl ActionRegistry {
    /// The kernel's own actions plus every action in the provider registry.
    ///
    /// # Errors
    ///
    /// `input`/`action_registry_invalid` for a duplicate name, or a
    /// provider's action whose name space is a noun of the kernel's or is
    /// not the name of a registered provider that declares it.
    pub fn build(providers: &a2m_provider::Registry) -> Result<Self, Failure> {
        let mut defs = kernel_definitions();
        let nouns: BTreeSet<String> = defs.iter().map(|def| def.name.noun().to_owned()).collect();
        let listing = providers.listing();
        for name in providers.actions() {
            let Some((action, effect)) = providers.action(name) else {
                continue;
            };
            let own = ActionName::new(name.as_str()).map_err(|_| {
                invalid(format!(
                    "The provider action `{}` is not a noun and a verb joined by one dot.",
                    codes::shown(name.as_str())
                ))
            })?;
            if nouns.contains(own.noun()) {
                return Err(invalid(format!(
                    "The provider action `{own}` uses `{}`, a noun of the kernel's own actions.",
                    own.noun()
                )));
            }
            let declared = listing.iter().any(|provider| {
                provider.registered
                    && provider.name.as_str() == own.noun()
                    && provider
                        .manifest
                        .actions
                        .iter()
                        .any(|declared| &declared.name == name)
            });
            if !declared {
                return Err(invalid(format!(
                    "The action `{own}` is outside the name space of a registered provider that declares it."
                )));
            }
            let reads = effect == EffectClass::Read;
            defs.push(ActionDef {
                summary: format!("An action the provider `{}` adds.", own.noun()),
                input: Input::of_schema(action.input_schema().clone()),
                output: action.output_schema().clone(),
                examples: Vec::new(),
                shape: if reads { Shape::Inspect } else { Shape::Change },
                effect: Effect::Fixed(effect),
                who: Who::Role(if reads { Role::Builder } else { Role::Admin }),
                listed: Listed::No,
                long_running: false,
                needs: if reads {
                    HealthNeed::Reads
                } else {
                    HealthNeed::Writes
                },
                person: PersonNeed::None,
                runs_in: RunsIn::Engine,
                faces: Faces::ALL,
                rest_alias: None,
                resource: None,
                phase: "provider",
                handler: Handler::Provider(name.clone()),
                name: own,
            });
        }
        Self::from_definitions(defs)
    }

    /// The kernel's own actions and no provider's. For generating the
    /// reference pages and the SDKs, which no installed provider may change.
    #[must_use]
    pub fn kernel_only() -> Self {
        let defs = kernel_definitions();
        let by_name = defs
            .iter()
            .enumerate()
            .map(|(at, def)| (def.name.clone(), at))
            .collect();
        Self {
            defs: Arc::new(defs),
            by_name: Arc::new(by_name),
        }
    }

    fn from_definitions(defs: Vec<ActionDef>) -> Result<Self, Failure> {
        let mut by_name = BTreeMap::new();
        for (at, def) in defs.iter().enumerate() {
            if by_name.insert(def.name.clone(), at).is_some() {
                return Err(invalid(format!("Two actions are named `{}`.", def.name)));
            }
            if def
                .input
                .fields
                .iter()
                .filter(|field| field.positional)
                .count()
                > 1
            {
                return Err(invalid(format!(
                    "The action `{}` declares more than one positional field.",
                    def.name
                )));
            }
            if def.faces.sdk && !def.faces.rest {
                // A client's method calls the action's REST route, and the
                // clients are generated from the routes' description.
                return Err(invalid(format!(
                    "The action `{}` has an SDK method but no REST route.",
                    def.name
                )));
            }
        }
        Ok(Self {
            defs: Arc::new(defs),
            by_name: Arc::new(by_name),
        })
    }

    /// The action of that name.
    #[must_use]
    pub fn get(&self, name: &ActionName) -> Option<&ActionDef> {
        self.by_name.get(name).and_then(|at| self.defs.get(*at))
    }

    /// The action written as `name`, when the text is a name and an action
    /// has it.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&ActionDef> {
        ActionName::new(name).ok().and_then(|name| self.get(&name))
    }

    /// Every action, in catalog order.
    pub fn iter(&self) -> impl Iterator<Item = &ActionDef> {
        self.defs.iter()
    }

    /// How many actions there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    /// Whether there are none. Never true: the kernel's own are always
    /// there.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// The names nearest to `wrong`, for a fix.
    #[must_use]
    pub fn nearest(&self, wrong: &str) -> Vec<&str> {
        codes::nearest(wrong, self.defs.iter().map(|def| def.name.as_str()))
    }

    /// The actions a built-in toolset lists up front, in catalog order: the
    /// three `action.*` tools first among those kept, then the marked
    /// actions until the toolset's cap is reached. An action that is not built
    /// is never listed up front (decision 0306); it is found by
    /// `action.search`. A marked action past the
    /// cap is reached through `action.run` (decision 0094).
    #[must_use]
    pub fn toolset(&self, toolset: BuiltInToolset) -> Vec<&ActionDef> {
        let marked: Vec<&ActionDef> = self
            .defs
            .iter()
            .filter(|def| def.listed.in_toolset(toolset) && def.built())
            .collect();
        let always = marked
            .iter()
            .filter(|def| def.listed == Listed::UseAndBuild)
            .count();
        let room = toolset.cap().saturating_sub(always);
        let mut taken = 0;
        marked
            .into_iter()
            .filter(|def| {
                if def.listed == Listed::UseAndBuild {
                    return true;
                }
                taken += 1;
                taken <= room
            })
            .collect()
    }

    /// The actions marked for a toolset that its cap leaves out of the list.
    #[must_use]
    pub fn past_cap(&self, toolset: BuiltInToolset) -> Vec<&ActionDef> {
        let listed: BTreeSet<&ActionName> = self
            .toolset(toolset)
            .into_iter()
            .map(|def| &def.name)
            .collect();
        self.defs
            .iter()
            .filter(|def| def.listed.in_toolset(toolset) && !listed.contains(&def.name))
            .collect()
    }

    /// The action catalog: every action as data, with a digest. The input
    /// to every generator.
    #[must_use]
    pub fn catalog(&self) -> Catalog {
        let actions: Vec<CatalogEntry> = self.defs.iter().map(CatalogEntry::of).collect();
        let digest =
            CanonicalJson::encode(&actions).map_or_else(|_| Digest::of(b""), |json| json.digest());
        Catalog {
            format: CATALOG_FORMAT,
            digest: digest.to_string(),
            actions,
        }
    }
}

fn kernel_definitions() -> Vec<ActionDef> {
    let mut defs = catalog::definitions();
    for def in &mut defs {
        if matches!(def.handler, Handler::NotBuilt)
            && let Some(handler) = handlers::handler_for(def.name.as_str())
        {
            def.handler = Handler::Kernel(handler);
        }
    }
    defs
}

fn invalid(message: String) -> Failure {
    codes::input(
        codes::REGISTRY_INVALID,
        message,
        "Rename the provider's action so that it starts with the provider's own name, then start again.",
    )
}

/// The action catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Catalog {
    /// The format of this record.
    pub format: u32,
    /// The digest of `actions`, as canonical JSON.
    pub digest: String,
    /// Every action, in catalog order.
    pub actions: Vec<CatalogEntry>,
}

/// One input field of a recorded action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogField {
    /// Its name.
    pub name: String,
    /// Its type, in a word.
    pub ty: &'static str,
    /// Whether it must be given.
    pub required: bool,
    /// Whether the command face takes it without a flag.
    pub positional: bool,
    /// What it is.
    pub help: String,
}

/// One action of the action catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogEntry {
    /// `noun.verb`.
    pub name: String,
    /// The one-line summary.
    pub summary: String,
    /// The input's fields.
    pub fields: Vec<CatalogField>,
    /// The input schema, as JSON Schema.
    pub input_schema: serde_json::Value,
    /// The output schema, as JSON Schema.
    pub output_schema: serde_json::Value,
    /// Example inputs.
    pub examples: Vec<serde_json::Value>,
    /// The action's shape.
    pub shape: &'static str,
    /// Its effect class, or `of_tool`.
    pub effect: String,
    /// The catalog's "Who" column.
    pub who: &'static str,
    /// The catalog's "Listed" column.
    pub listed: &'static str,
    /// Whether it returns a `run_id` at once.
    pub long_running: bool,
    /// `nothing`, `reads`, or `writes`.
    pub needs: &'static str,
    /// `none`, `presence`, or `presence_when_loosening`.
    pub person: &'static str,
    /// `engine` or `client`.
    pub runs_in: &'static str,
    /// The resource-shaped route, as method and path.
    pub rest_alias: Option<(String, String)>,
    /// Whether a handler is built.
    pub built: bool,
}

impl CatalogEntry {
    fn of(def: &ActionDef) -> Self {
        Self {
            name: def.name.to_string(),
            summary: def.summary.clone(),
            fields: def
                .input
                .fields
                .iter()
                .map(|field| CatalogField {
                    name: field.name.clone(),
                    ty: field.ty.as_str(),
                    required: field.required,
                    positional: field.positional,
                    help: field.help.clone(),
                })
                .collect(),
            input_schema: def.input.schema.to_json(),
            output_schema: def.output.to_json(),
            examples: def
                .examples
                .iter()
                .filter_map(|text| serde_json::from_str(text).ok())
                .collect(),
            shape: def.shape.as_str(),
            effect: match def.effect {
                Effect::Fixed(effect) => effect.to_string(),
                Effect::OfTool => "of_tool".to_owned(),
            },
            who: def.who.as_str(),
            listed: def.listed.as_str(),
            long_running: def.long_running,
            needs: match def.needs {
                HealthNeed::Nothing => "nothing",
                HealthNeed::Reads => "reads",
                HealthNeed::Writes => "writes",
            },
            person: match def.person {
                PersonNeed::None => "none",
                PersonNeed::Presence => "presence",
                PersonNeed::PresenceWhenLoosening => "presence_when_loosening",
            },
            runs_in: match def.runs_in {
                RunsIn::Engine => "engine",
                RunsIn::Client => "client",
            },
            rest_alias: def
                .rest_alias
                .map(|alias| (alias.method.to_owned(), alias.path.to_owned())),
            built: def.built(),
        }
    }
}

#[cfg(test)]
mod faces_tests {
    #![allow(clippy::unwrap_used, reason = "tests")]

    use super::{ActionRegistry, kernel_definitions};

    #[test]
    fn the_kernels_actions_make_a_registry() {
        assert!(ActionRegistry::from_definitions(kernel_definitions()).is_ok());
    }

    /// A client's method calls the action's REST route, so an SDK face
    /// without a REST face is refused (decision 0420).
    #[test]
    fn an_action_with_an_sdk_method_and_no_rest_route_is_refused() {
        let mut defs = kernel_definitions();
        defs[0].faces.rest = false;
        let name = defs[0].name.to_string();
        let refused = ActionRegistry::from_definitions(defs.clone()).unwrap_err();
        assert!(refused.message().contains(&name), "{}", refused.message());
        defs[0].faces.sdk = false;
        assert!(ActionRegistry::from_definitions(defs).is_ok());
    }

    /// A resource's URI fills its action's fields from the template's
    /// segments and query, and anything else is no resource (decision
    /// 0999): nothing is decoded or guessed.
    #[test]
    fn a_resource_uri_fills_its_actions_fields_and_nothing_else() {
        let registry = ActionRegistry::kernel_only();
        let faces = crate::faces::mcp_resources(&registry);
        assert_eq!(
            faces
                .iter()
                .map(|face| (face.action.as_str(), face.uri_template.as_str()))
                .collect::<Vec<_>>(),
            [("watch.changes", "a2m://watches/{watch_id}/changes{?after}")]
        );
        let (def, input) = registry
            .by_resource_uri("a2m://watches/watch_01/changes")
            .unwrap();
        assert_eq!(def.name.as_str(), "watch.changes");
        assert_eq!(
            serde_json::Value::Object(input),
            serde_json::json!({"watch_id": "watch_01"})
        );
        let (_, input) = registry
            .by_resource_uri("a2m://watches/watch_01/changes?after=12")
            .unwrap();
        assert_eq!(
            serde_json::Value::Object(input),
            serde_json::json!({"watch_id": "watch_01", "after": 12})
        );
        for uri in [
            "a2m://watches/watch_01",
            "a2m://watches/watch_01/changes/more",
            "a2m://watches//changes",
            "a2m://watches/a%2Fb/changes",
            "a2m://watches/watch_01/changes?limit=3",
            "a2m://watches/watch_01/changes?after=1&after=2",
            "a2m://watches/watch_01/changes?watch_id=other",
            "a2m://watches/watch_01/changes?after=ten",
            "a2m://watches/watch_01/changes?after",
            "file:///watches/watch_01/changes",
        ] {
            assert!(registry.by_resource_uri(uri).is_none(), "{uri}");
        }
        // The URI every `watch.*` result names is one the template matches.
        let id = a2m_ledger::WatchId::from_bytes([3; 16]);
        assert!(
            registry
                .by_resource_uri(&crate::watch_resource(id))
                .is_some()
        );
    }
}
