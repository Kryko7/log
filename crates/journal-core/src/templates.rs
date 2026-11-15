use std::fs;
use std::path::Path;

use journal_types::Template;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::markdown::split_frontmatter;

#[derive(Debug, Serialize, Deserialize)]
struct TemplateFrontmatter {
    id: String,
    name: String,
    description: String,
    category: String,
    #[serde(default)]
    default_tags: Vec<String>,
    #[serde(default)]
    default_mood: Option<String>,
    #[serde(default)]
    prompts: Vec<String>,
}

/// Built-in starter editorial Markdown templates seeded into `templates/` on vault creation.
pub fn starter_templates() -> Vec<Template> {
    vec![
        Template {
            id: "evening-reflection".to_string(),
            name: "Evening Reflection".to_string(),
            description: "Unwind the day's thread with quiet honesty before turning down the lamp.".to_string(),
            category: "Daily".to_string(),
            default_tags: vec!["evening".to_string(), "reflection".to_string()],
            default_mood: Some("reflective".to_string()),
            prompts: vec![
                "What small sensory detail from today deserves to be remembered a year from now?".to_string(),
                "Where did your attention linger, and what felt unfinished?".to_string(),
                "What can you gently set down before sleep?".to_string(),
            ],
            body_markdown: "\
## Dusk & Unwinding

*A small detail worth keeping from today:*


### What lingered in attention


### Something to set down tonight
"
            .to_string(),
        },
        Template {
            id: "morning-pages".to_string(),
            name: "Morning Pages".to_string(),
            description: "Unfiltered first-light prose to clear the mental desk before the day begins.".to_string(),
            category: "Daily".to_string(),
            default_tags: vec!["morning".to_string(), "pages".to_string()],
            default_mood: Some("calm".to_string()),
            prompts: vec![
                "What is sitting closest to the surface as you wake?".to_string(),
                "If today moved at half its usual hurry, what would matter most?".to_string(),
                "One quiet intention for the hours ahead.".to_string(),
            ],
            body_markdown: "\
## First Light

*Before checking the noise of the world, what is already on your mind?*


### The pace of today

- **One quiet intention:** 
- **One thing to protect:** 
"
            .to_string(),
        },
        Template {
            id: "field-travel-notes".to_string(),
            name: "Field & Travel Notes".to_string(),
            description: "Dispatch-style observations of place, weather, overheard fragments, and routes.".to_string(),
            category: "Travel".to_string(),
            default_tags: vec!["field-notes".to_string(), "travel".to_string()],
            default_mood: Some("curious".to_string()),
            prompts: vec![
                "What does the air smell and sound like right here?".to_string(),
                "Which street, room, or horizon caught your eye?".to_string(),
                "An exchange or small discovery worth sketching in words.".to_string(),
            ],
            body_markdown: "\
## Field Dispatch

**Where I am sitting:**  
**Light & atmosphere:** 

### Observations & fragments


### Route / Next stop
"
            .to_string(),
        },
        Template {
            id: "quiet-gratitude".to_string(),
            name: "Quiet Gratitude".to_string(),
            description: "Three unhurried anchors of appreciation, ordinary kindness, and stillness.".to_string(),
            category: "Mindfulness".to_string(),
            default_tags: vec!["gratitude".to_string(), "quiet".to_string()],
            default_mood: Some("grateful".to_string()),
            prompts: vec![
                "An ordinary comfort you almost overlooked today.".to_string(),
                "A person whose warmth or steadiness made a difference.".to_string(),
                "A moment of stillness you want to revisit.".to_string(),
            ],
            body_markdown: "\
## Quiet Anchors

1. **An ordinary comfort:** 
2. **A steadying presence:** 
3. **A moment of stillness:** 

*Notes in the margin:*
"
            .to_string(),
        },
    ]
}

/// Seed the 4 starter templates into `templates/` if no template files exist yet.
pub fn seed_templates(templates_dir: &Path) -> Result<()> {
    fs::create_dir_all(templates_dir)?;

    let has_any = fs::read_dir(templates_dir)?
        .filter_map(|e| e.ok())
        .any(|entry| {
            let p = entry.path();
            p.is_file()
                && matches!(
                    p.extension().and_then(|s| s.to_str()),
                    Some("md") | Some("json")
                )
        });

    if has_any {
        return Ok(());
    }

    for tpl in starter_templates() {
        let fm = TemplateFrontmatter {
            id: tpl.id.clone(),
            name: tpl.name.clone(),
            description: tpl.description.clone(),
            category: tpl.category.clone(),
            default_tags: tpl.default_tags.clone(),
            default_mood: tpl.default_mood.clone(),
            prompts: tpl.prompts.clone(),
        };
        let raw_yaml = serde_yaml::to_string(&fm)?;
        let yaml = raw_yaml
            .strip_prefix("---\r\n")
            .or_else(|| raw_yaml.strip_prefix("---\n"))
            .unwrap_or(&raw_yaml)
            .trim_end_matches(['\r', '\n']);

        let content = format!("---\n{yaml}\n---\n\n{}", tpl.body_markdown);
        let target = templates_dir.join(format!("{}.md", tpl.id));
        fs::write(target, content)?;
    }

    Ok(())
}

/// Load all templates from `templates/`, ordered canonically.
pub fn load_templates(templates_dir: &Path) -> Result<Vec<Template>> {
    if !templates_dir.exists() {
        return Ok(Vec::new());
    }

    let mut templates = Vec::new();
    for entry in fs::read_dir(templates_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        match path.extension().and_then(|s| s.to_str()) {
            Some("md") => {
                let raw = fs::read_to_string(&path)?;
                let (yaml, body) = split_frontmatter(&raw)?;
                let fm: TemplateFrontmatter = serde_yaml::from_str(yaml)?;
                templates.push(Template {
                    id: fm.id,
                    name: fm.name,
                    description: fm.description,
                    category: fm.category,
                    default_tags: fm.default_tags,
                    default_mood: fm.default_mood,
                    body_markdown: body.to_string(),
                    prompts: fm.prompts,
                });
            }
            Some("json") => {
                let raw = fs::read_to_string(&path)?;
                let tpl: Template = serde_json::from_str(&raw)?;
                templates.push(tpl);
            }
            _ => {}
        }
    }

    let order = [
        "evening-reflection",
        "morning-pages",
        "field-travel-notes",
        "quiet-gratitude",
    ];
    templates.sort_by(|a, b| {
        let pos_a = order.iter().position(|id| *id == a.id).unwrap_or(usize::MAX);
        let pos_b = order.iter().position(|id| *id == b.id).unwrap_or(usize::MAX);
        pos_a.cmp(&pos_b).then_with(|| a.name.cmp(&b.name))
    });

    Ok(templates)
}
