use std::collections::{BTreeSet, HashMap};

pub struct QuoteForm {
    pub id: Option<String>,
    pub discord_id: String,
    pub title: String,
    pub description: String,
    pub is_visible: bool,
    pub sections: Vec<QuoteFormSection>,
    pub error: Option<String>,
}

pub struct QuoteFormSection {
    pub title: String,
    pub description: String,
    pub min_hours: String,
    pub max_hours: String,
    pub price_euros: String,
}

pub(super) struct QuoteInput {
    pub discord_id: String,
    pub title: String,
    pub description: String,
    pub is_visible: bool,
    pub sections: Vec<SectionInput>,
}

pub(super) struct SectionInput {
    pub title: String,
    pub description: String,
    pub min_minutes: i32,
    pub max_minutes: Option<i32>,
    pub price_cents: i64,
}

impl QuoteForm {
    pub fn empty() -> Self {
        Self {
            id: None,
            discord_id: String::new(),
            title: String::new(),
            description: String::new(),
            is_visible: false,
            sections: vec![QuoteFormSection::empty()],
            error: None,
        }
    }

    pub fn from_fields(fields: &HashMap<String, String>, id: Option<String>) -> Self {
        let indexes = fields
            .keys()
            .filter_map(|key| key.strip_prefix("sections."))
            .filter_map(|key| key.split_once('.'))
            .filter_map(|(index, _)| index.parse().ok())
            .collect::<BTreeSet<usize>>();
        let sections = indexes
            .into_iter()
            .map(|index| QuoteFormSection {
                title: field(fields, &format!("sections.{index}.title")),
                description: field(fields, &format!("sections.{index}.description")),
                min_hours: field(fields, &format!("sections.{index}.min_hours")),
                max_hours: field(fields, &format!("sections.{index}.max_hours")),
                price_euros: field(fields, &format!("sections.{index}.price_euros")),
            })
            .collect();

        Self {
            id,
            discord_id: field(fields, "discord_id"),
            title: field(fields, "title"),
            description: field(fields, "description"),
            is_visible: fields.contains_key("is_visible"),
            sections,
            error: None,
        }
    }

    pub(super) fn validate(&mut self) -> Result<QuoteInput, ()> {
        let discord_id = self.discord_id.trim();
        if !(17..=20).contains(&discord_id.len())
            || !discord_id.bytes().all(|byte| byte.is_ascii_digit())
        {
            return self.invalid("Enter a valid Discord ID.");
        }
        if self.title.trim().is_empty() || self.description.trim().is_empty() {
            return self.invalid("Title and description are required.");
        }
        if self.sections.is_empty() {
            return self.invalid("Add at least one section.");
        }

        let mut sections = Vec::with_capacity(self.sections.len());
        for section in &self.sections {
            let minimum = positive_i32(&section.min_hours);
            let maximum = optional_i32(&section.max_hours);
            let price = non_negative_i64(&section.price_euros);
            if section.title.trim().is_empty()
                || section.description.trim().is_empty()
                || minimum.is_none()
                || maximum.is_none()
                || maximum
                    .flatten()
                    .is_some_and(|value| value < minimum.unwrap_or(0))
                || price.is_none()
            {
                return self.invalid(
                    "Complete every section with valid whole hours and a non-negative price.",
                );
            }
            let Some(min_minutes) = minimum.and_then(|value| value.checked_mul(60)) else {
                return self.invalid("The minimum estimate is too large.");
            };
            let max_minutes = match maximum.flatten() {
                Some(value) => match value.checked_mul(60) {
                    Some(value) => Some(value),
                    None => return self.invalid("The maximum estimate is too large."),
                },
                None => None,
            };
            let Some(price_cents) = price.and_then(|value| value.checked_mul(100)) else {
                return self.invalid("The price is too large.");
            };
            sections.push(SectionInput {
                title: section.title.trim().into(),
                description: section.description.trim().into(),
                min_minutes,
                max_minutes,
                price_cents,
            });
        }

        Ok(QuoteInput {
            discord_id: discord_id.into(),
            title: self.title.trim().into(),
            description: self.description.trim().into(),
            is_visible: self.is_visible,
            sections,
        })
    }

    fn invalid<T>(&mut self, message: &str) -> Result<T, ()> {
        self.error = Some(message.into());
        Err(())
    }
}

impl QuoteFormSection {
    pub(super) fn empty() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            min_hours: String::new(),
            max_hours: String::new(),
            price_euros: String::new(),
        }
    }
}

fn field(fields: &HashMap<String, String>, name: &str) -> String {
    fields.get(name).cloned().unwrap_or_default()
}

fn positive_i32(value: &str) -> Option<i32> {
    value.parse().ok().filter(|value| *value > 0)
}

fn optional_i32(value: &str) -> Option<Option<i32>> {
    if value.trim().is_empty() {
        Some(None)
    } else {
        positive_i32(value).map(Some)
    }
}

fn non_negative_i64(value: &str) -> Option<i64> {
    value.parse().ok().filter(|value| *value >= 0)
}
