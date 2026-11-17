//! What is installed: every pack and provider, and one in full.

use a2m_core::Failure;
use a2m_provider::{ProviderKind, ProviderListing, SettingType};
use serde_json::{Value as Json, json};

use super::handler;
use crate::codes;
use crate::dispatch::{Next, Output};

fn problem(failure: &Failure) -> Json {
    json!({
        "class": failure.class().to_string(),
        "code": failure.code().to_string(),
        "message": failure.message(),
        "fix": failure.next_step(),
    })
}

const fn kind(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Pack => "pack",
        ProviderKind::Plugin => "plugin",
    }
}

fn listed(provider: &ProviderListing) -> Json {
    json!({
        "name": provider.name.as_str(),
        "version": provider.version.to_string(),
        "kind": kind(provider.kind),
        "availability": provider.availability.as_str(),
        "registered": provider.registered,
        "fix": provider.fix,
        "problems": provider.problems.iter().map(problem).collect::<Vec<_>>(),
    })
}

handler!(
    /// `provider.list`: every pack and provider, in order of name.
    provider_list,
    |kernel, _call| {
        let registry = kernel.providers();
        let mut providers: Vec<Json> = registry.listing().iter().map(listed).collect();
        // A provider whose manifest was refused registers nothing and has
        // no listing: it is named all the same, with why, as far as its
        // manifest's text names it.
        providers.extend(registry.refused().iter().map(|refused| {
            json!({
                "name": refused.name.as_deref().map_or_else(|| "(unnamed)".to_owned(), codes::shown),
                "version": refused.version.as_deref().map(codes::shown).unwrap_or_default(),
                "kind": kind(refused.kind),
                "availability": "refused",
                "registered": false,
                "fix": refused.problem.next_step(),
                "problems": [problem(&refused.problem)],
            })
        }));
        Ok(Output::ok(json!({"providers": providers})).then(Next::new(
            "provider.describe",
            json!({"provider": "<name>"}),
            "to read one provider's manifest",
        )))
    }
);

handler!(
    /// `provider.describe`: one provider's manifest, as data.
    provider_describe,
    |kernel, call| {
        let name = call.required("provider")?;
        let listing = kernel.providers().listing();
        let Some(provider) = listing
            .iter()
            .find(|provider| provider.name.as_str() == name)
        else {
            let near = codes::nearest(name, listing.iter().map(|provider| provider.name.as_str()));
            return Err(codes::input(
                codes::NOT_FOUND,
                format!("No provider is named `{}`.", codes::shown(name)),
                if near.is_empty() {
                    "Call `provider.list` to see what is installed.".to_owned()
                } else {
                    format!("Use one of: {}.", near.join(", "))
                },
            ));
        };
        let manifest = &provider.manifest;
        Ok(Output::ok(json!({
            "name": provider.name.as_str(),
            "version": provider.version.to_string(),
            "kind": kind(provider.kind),
            "publisher": manifest.provider.publisher,
            "availability": provider.availability.as_str(),
            "registered": provider.registered,
            "fix": provider.fix,
            "digest": manifest.digest.to_string(),
            "modes": manifest.modes.iter().map(|mode| mode.name.to_string()).collect::<Vec<_>>(),
            "surface_kinds": manifest
                .surface_kinds
                .iter()
                .map(|kind| kind.name.to_string())
                .collect::<Vec<_>>(),
            "actions": manifest
                .actions
                .iter()
                .map(|action| action.name.to_string())
                .collect::<Vec<_>>(),
            "settings": manifest.settings.iter().map(|setting| json!({
                "key": format!("packs.{}.{}", provider.name, setting.key),
                "type": match setting.ty {
                    SettingType::Bool => "bool",
                    SettingType::Int => "int",
                    SettingType::Text => "text",
                    SettingType::Choice => "choice",
                    SettingType::List => "list",
                },
                "protected": setting.protected,
                "doc": setting.doc,
            })).collect::<Vec<_>>(),
            "requirements": manifest.requirements.len(),
            "platforms": manifest.platforms,
            "problems": provider.problems.iter().map(problem).collect::<Vec<_>>(),
        })))
    }
);
