pub const NOTIFY_URI: &str = "https://notify.walletconnect.com";
pub const ABILITY_NAMESPACE_MANAGE: &str = "manage";
pub const ABILITY_ABILITY_ALL_APPS_MAGIC: &str = "all-apps";
pub const ABILITY_ABILITY_SUFFIX: &str = "-notifications";

pub mod test_utils {
    use {
        super::*,
        crate::siwx::erc5573::{Ability, ReCapDetailsObject},
        serde_json::{Map, Value},
        std::collections::HashMap,
    };

    pub fn build_recap_details_object(domain: Option<&str>) -> ReCapDetailsObject {
        ReCapDetailsObject {
            att: HashMap::from_iter([(
                NOTIFY_URI.to_owned(),
                HashMap::from_iter([(
                    Ability {
                        namespace: ABILITY_NAMESPACE_MANAGE.to_owned(),
                        name: format!(
                            "{}{ABILITY_ABILITY_SUFFIX}",
                            domain.unwrap_or(ABILITY_ABILITY_ALL_APPS_MAGIC)
                        ),
                    },
                    vec![Value::Object(Map::from_iter([]))],
                )]),
            )]),
        }
    }
}
