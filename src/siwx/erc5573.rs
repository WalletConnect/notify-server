use {
    data_encoding::{DecodeError, BASE64URL_NOPAD},
    itertools::Itertools,
    once_cell::sync::Lazy,
    regex::Regex,
    serde::{Deserialize, Serialize},
    serde_json::Value,
    std::collections::HashMap,
    thiserror::Error,
};

// https://eips.ethereum.org/EIPS/eip-5573

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Ability {
    pub namespace: String,
    pub name: String,
}

impl Serialize for Ability {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("{}/{}", self.namespace, self.name))
    }
}

#[derive(Debug, Error)]
#[error("Ability string invalid")]
pub struct AbilityParseError;

impl<'a> Deserialize<'a> for Ability {
    fn deserialize<D: serde::Deserializer<'a>>(deserializer: D) -> Result<Self, D::Error> {
        static REGEX: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"^([a-zA-Z0-9.*_+-]+)\/([a-zA-z0-9.*_+-]+)$")
                .expect("Error should be caught in test cases")
        });

        let ability = String::deserialize(deserializer)?;

        if let Some(caps) = REGEX.captures(&ability) {
            let (_, [namespace, ability]) = caps.extract();
            Ok(Ability {
                namespace: namespace.to_owned(),
                name: ability.to_owned(),
            })
        } else {
            Err(serde::de::Error::custom(AbilityParseError))
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct ReCapDetailsObject {
    pub att: HashMap<String, HashMap<Ability, Vec<Value>>>,
}

#[derive(Debug, Error)]
pub enum RecapParseError {
    #[error("Decode error: {0}")]
    Decode(DecodeError),

    #[error("JSON deserialization error: {0}")]
    Json(serde_json::Error),

    #[error("Invalid URI: {0}")]
    InvalidUri(String),
}

pub const RECAP_URI_PREFIX: &str = "urn:recap:";

pub fn parse_recap(
    last_resource: Option<&str>,
) -> Result<Option<ReCapDetailsObject>, RecapParseError> {
    last_resource
        .and_then(|resource| {
            resource
                .strip_prefix(RECAP_URI_PREFIX)
                .map(|recap_encoded| {
                    BASE64URL_NOPAD
                        .decode(recap_encoded.as_bytes())
                        .map_err(RecapParseError::Decode)
                        .and_then(|recap_decoded| {
                            serde_json::from_slice::<ReCapDetailsObject>(&recap_decoded)
                                .map_err(RecapParseError::Json)
                        })
                        .and_then(|recap| {
                            static URI_REGEX: Lazy<Regex> = Lazy::new(|| {
                                Regex::new(r"^.+:.*$")
                                    .expect("Safe unwrap: Error should be caught in test cases")
                            });
                            for uri in recap.att.keys() {
                                if URI_REGEX.captures(uri).is_none()
                                    // https://walletconnect.slack.com/archives/C03RVH94K5K/p1713799617021109?thread_ts=1712839862.846379&cid=C03RVH94K5K
                                    && uri != "eip155"
                                {
                                    return Err(RecapParseError::InvalidUri(uri.clone()));
                                }
                            }
                            Ok(recap)
                        })
                })
        })
        .transpose()
}

pub fn build_statement(recap: &ReCapDetailsObject) -> String {
    let mut statement =
        "I further authorize the stated URI to perform the following actions on my behalf:"
            .to_owned();
    let mut ability_index = 1;
    for (uri, abilities) in recap.att.iter().sorted_by_key(|(uri, _)| *uri) {
        let mut ability_groups = HashMap::with_capacity(abilities.len());
        for ability in abilities.keys() {
            ability_groups
                .entry(ability.namespace.clone())
                .or_insert_with(Vec::new)
                .push(ability.name.clone());
        }
        for (namespace, abilities) in ability_groups
            .into_iter()
            .sorted_by_key(|(namespace, _)| namespace.clone())
        {
            let abilities = abilities
                .into_iter()
                .sorted()
                .map(|ability| format!(" '{}'", ability))
                .join(",");
            statement.push_str(&format!(
                " ({ability_index}) '{namespace}':{abilities} for '{uri}'."
            ));
            ability_index += 1;
        }
    }
    statement
}

pub mod test_utils {
    use {
        super::{ReCapDetailsObject, RECAP_URI_PREFIX},
        data_encoding::BASE64URL_NOPAD,
    };

    pub fn encode_recaip_uri(recap: &ReCapDetailsObject) -> String {
        let payload = BASE64URL_NOPAD.encode(
            serde_json::to_string(recap)
                .expect("Encoding as JSON should not fail")
                .as_bytes(),
        );
        format!("{RECAP_URI_PREFIX}{payload}")
    }
}

#[cfg(test)]
mod tests {
    use {
        self::test_utils::encode_recaip_uri,
        super::*,
        once_cell::sync::Lazy,
        serde_json::{json, Map, Number},
        std::collections::HashSet,
    };

    static RECAP_RESOURCE: &str = "urn:recap:eyJhdHQiOnsiaHR0cHM6Ly9leGFtcGxlLmNvbS9waWN0dXJlcy8iOnsiY3J1ZC9kZWxldGUiOlt7fV0sImNydWQvdXBkYXRlIjpbe31dLCJvdGhlci9hY3Rpb24iOlt7fV19LCJtYWlsdG86dXNlcm5hbWVAZXhhbXBsZS5jb20iOnsibXNnL3JlY2VpdmUiOlt7Im1heF9jb3VudCI6NSwidGVtcGxhdGVzIjpbIm5ld3NsZXR0ZXIiLCJtYXJrZXRpbmciXX1dLCJtc2cvc2VuZCI6W3sidG8iOiJzb21lb25lQGVtYWlsLmNvbSJ9LHsidG8iOiJqb2VAZW1haWwuY29tIn1dfX0sInByZiI6WyJ6ZGo3V2o2Rk5TNHJVVWJzaUp2amp4Y3NOcVpkRENTaVlSOHNLUVhmb1BmcFNadUF3Il19";
    static RECAP_TEST: Lazy<ReCapDetailsObject> = Lazy::new(|| ReCapDetailsObject {
        att: HashMap::from([
            (
                "https://example.com/pictures/".to_owned(),
                HashMap::from([
                    (
                        Ability {
                            namespace: "crud".to_owned(),
                            name: "delete".to_owned(),
                        },
                        vec![Value::Object(Map::new())],
                    ),
                    (
                        Ability {
                            namespace: "crud".to_owned(),
                            name: "update".to_owned(),
                        },
                        vec![Value::Object(Map::new())],
                    ),
                    (
                        Ability {
                            namespace: "other".to_owned(),
                            name: "action".to_owned(),
                        },
                        vec![Value::Object(Map::new())],
                    ),
                ]),
            ),
            (
                "mailto:username@example.com".to_owned(),
                HashMap::from([
                    (
                        Ability {
                            namespace: "msg".to_owned(),
                            name: "receive".to_owned(),
                        },
                        vec![Value::Object(Map::from_iter([
                            ("max_count".to_string(), Value::Number(Number::from(5))),
                            (
                                "templates".to_string(),
                                Value::Array(vec![
                                    Value::String("newsletter".to_owned()),
                                    Value::String("marketing".to_owned()),
                                ]),
                            ),
                        ]))],
                    ),
                    (
                        Ability {
                            namespace: "msg".to_owned(),
                            name: "send".to_owned(),
                        },
                        vec![
                            Value::Object(Map::from_iter([(
                                "to".to_string(),
                                Value::String("someone@email.com".to_owned()),
                            )])),
                            Value::Object(Map::from_iter([(
                                "to".to_string(),
                                Value::String("joe@email.com".to_owned()),
                            )])),
                        ],
                    ),
                ]),
            ),
        ]),
    });

    #[test]
    fn json() {
        let json = json!({
            "att": {
                "https://example.com/pictures/": {
                    "crud/delete": [{}],
                    "crud/update": [{}],
                    "other/action": [{}]
                },
                "mailto:username@example.com": {
                    "msg/receive": [{
                        "max_count": 5,
                        "templates": ["newsletter", "marketing"]
                    }],
                    "msg/send": [{ "to": "someone@email.com" }, { "to": "joe@email.com" }]
                }
            },
        });
        let recap = serde_json::from_value::<ReCapDetailsObject>(json).unwrap();
        let expected = RECAP_TEST.clone();
        assert_eq!(recap, expected);
    }

    #[test]
    fn optional() {
        assert_eq!(parse_recap(None).unwrap(), None,);
    }

    #[test]
    fn parse() {
        assert_eq!(
            parse_recap(Some(RECAP_RESOURCE)).unwrap(),
            Some(RECAP_TEST.clone()),
        );
    }

    #[test]
    fn parse_ignores_non_recaps() {
        assert_eq!(parse_recap(Some("junk")).unwrap(), None);
    }

    #[test]
    fn parse_decoding_failed() {
        assert!(matches!(
            parse_recap(Some(&format!("{RECAP_RESOURCE}="))),
            Err(RecapParseError::Decode(_))
        ));
    }

    #[test]
    fn parse_json_failed() {
        assert!(matches!(
            parse_recap(Some(&format!("{RECAP_RESOURCE}junk"))),
            Err(RecapParseError::Json(_))
        ));
    }

    #[test]
    fn parse_uri_validation_failed() {
        assert!(matches!(
            parse_recap(Some(&format!(
                "urn:recap:{}",
                BASE64URL_NOPAD.encode(r#"{"att":{"invalid-uri":{}}}"#.as_bytes())
            ))),
            Err(RecapParseError::InvalidUri(_))
        ));
    }

    #[test]
    fn abilities() {
        let recap = serde_json::from_value::<ReCapDetailsObject>(json!({
          "att": {
            "https://example1.com": {
              "crud/read": [{}],
              "crud/update": [{
                "max_times": 1
              }]
            },
            "https://example2.com": {
              "crud/delete": [{}]
            }
          },
        }))
        .unwrap();
        assert_eq!(
            recap
                .att
                .get("https://example1.com")
                .unwrap()
                .keys()
                .collect::<HashSet<_>>(),
            HashSet::from([
                &Ability {
                    namespace: "crud".to_owned(),
                    name: "read".to_owned(),
                },
                &Ability {
                    namespace: "crud".to_owned(),
                    name: "update".to_owned(),
                }
            ])
        );
        assert_eq!(
            recap
                .att
                .get("https://example2.com")
                .unwrap()
                .keys()
                .collect::<HashSet<_>>(),
            HashSet::from([&Ability {
                namespace: "crud".to_owned(),
                name: "delete".to_owned(),
            }])
        );
    }

    #[test]
    fn allow_non_standard_eip155_att_key() {
        // https://walletconnect.slack.com/archives/C03RVH94K5K/p1713799617021109?thread_ts=1712839862.846379&cid=C03RVH94K5K
        let recap = serde_json::from_value::<ReCapDetailsObject>(json!({
          "att": {
            "eip155": {
              "request/eth_sendRawTransaction": [
                {
                  "chains": [
                    "eip155:1"
                  ]
                }
              ],
              "request/eth_sendTransaction": [
                {
                  "chains": [
                    "eip155:1"
                  ]
                }
              ],
              "request/eth_sign": [
                {
                  "chains": [
                    "eip155:1"
                  ]
                }
              ],
              "request/eth_signTransaction": [
                {
                  "chains": [
                    "eip155:1"
                  ]
                }
              ],
              "request/eth_signTypedData": [
                {
                  "chains": [
                    "eip155:1"
                  ]
                }
              ],
              "request/eth_signTypedData_v3": [
                {
                  "chains": [
                    "eip155:1"
                  ]
                }
              ],
              "request/eth_signTypedData_v4": [
                {
                  "chains": [
                    "eip155:1"
                  ]
                }
              ],
              "request/personal_sign": [
                {
                  "chains": [
                    "eip155:1"
                  ]
                }
              ]
            },
            "https://notify.walletconnect.com": {
              "manage/all-apps-notifications": [
                {}
              ]
            }
          }
        }))
        .unwrap();
        let encoded = encode_recaip_uri(&recap);
        let parsed = parse_recap(Some(&encoded)).unwrap().unwrap();
        parsed.att.get("eip155").unwrap();
    }

    #[test]
    fn ability_parse() {
        assert_eq!(
            serde_json::from_value::<Ability>(json!("crud/read")).unwrap(),
            Ability {
                namespace: "crud".to_owned(),
                name: "read".to_owned(),
            }
        );
    }

    #[test]
    fn ability_parse_error() {
        assert_eq!(
            serde_json::from_value::<Ability>(json!(""))
                .unwrap_err()
                .to_string(),
            AbilityParseError.to_string()
        );
        assert_eq!(
            serde_json::from_value::<Ability>(json!("/"))
                .unwrap_err()
                .to_string(),
            AbilityParseError.to_string()
        );
        assert_eq!(
            serde_json::from_value::<Ability>(json!("abc"))
                .unwrap_err()
                .to_string(),
            AbilityParseError.to_string()
        );
        assert_eq!(
            serde_json::from_value::<Ability>(json!("crud//read"))
                .unwrap_err()
                .to_string(),
            AbilityParseError.to_string()
        );
        assert_eq!(
            serde_json::from_value::<Ability>(json!("crud/read/"))
                .unwrap_err()
                .to_string(),
            AbilityParseError.to_string()
        );
        assert_eq!(
            serde_json::from_value::<Ability>(json!("crudread/"))
                .unwrap_err()
                .to_string(),
            AbilityParseError.to_string()
        );
        assert_eq!(
            serde_json::from_value::<Ability>(json!("crud/read!"))
                .unwrap_err()
                .to_string(),
            AbilityParseError.to_string()
        );
    }

    #[test]
    fn statement1() {
        let recap_string = "urn:recap:eyJhdHQiOnsiaHR0cHM6Ly9leGFtcGxlLmNvbS9waWN0dXJlcy8iOnsiY3J1ZC9kZWxldGUiOlt7fV0sImNydWQvdXBkYXRlIjpbe31dLCJvdGhlci9hY3Rpb24iOlt7fV19LCJtYWlsdG86dXNlcm5hbWVAZXhhbXBsZS5jb20iOnsibXNnL3JlY2VpdmUiOlt7Im1heF9jb3VudCI6NSwidGVtcGxhdGVzIjpbIm5ld3NsZXR0ZXIiLCJtYXJrZXRpbmciXX1dLCJtc2cvc2VuZCI6W3sidG8iOiJzb21lb25lQGVtYWlsLmNvbSJ9LHsidG8iOiJqb2VAZW1haWwuY29tIn1dfX0sInByZiI6WyJ6ZGo3V2o2Rk5TNHJVVWJzaUp2amp4Y3NOcVpkRENTaVlSOHNLUVhmb1BmcFNadUF3Il19";
        let expected_statement = "I further authorize the stated URI to perform the following actions on my behalf: (1) 'crud': 'delete', 'update' for 'https://example.com/pictures/'. (2) 'other': 'action' for 'https://example.com/pictures/'. (3) 'msg': 'receive', 'send' for 'mailto:username@example.com'.";
        let recap = parse_recap(Some(recap_string)).unwrap().unwrap();
        let statement = build_statement(&recap);
        assert_eq!(statement, expected_statement);
    }

    #[test]
    fn statement2() {
        let recap_string = "urn:recap:eyJhdHQiOnsiaHR0cHM6Ly9leGFtcGxlLmNvbSI6eyJleGFtcGxlL2FwcGVuZCI6W10sImV4YW1wbGUvcmVhZCI6W10sIm90aGVyL2FjdGlvbiI6W119LCJteTpyZXNvdXJjZTp1cmkuMSI6eyJleGFtcGxlL2FwcGVuZCI6W10sImV4YW1wbGUvZGVsZXRlIjpbXX0sIm15OnJlc291cmNlOnVyaS4yIjp7ImV4YW1wbGUvYXBwZW5kIjpbXX0sIm15OnJlc291cmNlOnVyaS4zIjp7ImV4YW1wbGUvYXBwZW5kIjpbXX19LCJwcmYiOltdfQ";
        let expected_statement = "I further authorize the stated URI to perform the following actions on my behalf: (1) 'example': 'append', 'read' for 'https://example.com'. (2) 'other': 'action' for 'https://example.com'. (3) 'example': 'append', 'delete' for 'my:resource:uri.1'. (4) 'example': 'append' for 'my:resource:uri.2'. (5) 'example': 'append' for 'my:resource:uri.3'.";
        let recap = parse_recap(Some(recap_string)).unwrap().unwrap();
        let statement = build_statement(&recap);
        assert_eq!(statement, expected_statement);
    }

    #[test]
    fn statement_empty_recaps() {
        let expected_statement =
            "I further authorize the stated URI to perform the following actions on my behalf:";
        let recap = ReCapDetailsObject {
            att: HashMap::from([]),
        };
        let statement = build_statement(&recap);
        assert_eq!(statement, expected_statement);
    }

    #[test]
    fn statement_no_abilities() {
        let expected_statement =
            "I further authorize the stated URI to perform the following actions on my behalf:";
        let recap = ReCapDetailsObject {
            att: HashMap::from([("uri1".to_owned(), HashMap::from([]))]),
        };
        let statement = build_statement(&recap);
        assert_eq!(statement, expected_statement);
    }

    #[test]
    fn statement_1_uri_1_namespace_1_ability() {
        let expected_statement = "I further authorize the stated URI to perform the following actions on my behalf: (1) 'namespace1': 'ability1' for 'uri1'.";
        let recap = ReCapDetailsObject {
            att: HashMap::from([(
                "uri1".to_owned(),
                HashMap::from([(
                    Ability {
                        namespace: "namespace1".to_owned(),
                        name: "ability1".to_owned(),
                    },
                    vec![Value::Object(Map::new())],
                )]),
            )]),
        };
        let statement = build_statement(&recap);
        assert_eq!(statement, expected_statement);
    }

    #[test]
    fn statement_1_uri_1_namespace_2_ability() {
        let expected_statement = "I further authorize the stated URI to perform the following actions on my behalf: (1) 'namespace1': 'ability1', 'ability2' for 'uri1'.";
        let recap = ReCapDetailsObject {
            att: HashMap::from([(
                "uri1".to_owned(),
                HashMap::from([
                    (
                        Ability {
                            namespace: "namespace1".to_owned(),
                            name: "ability1".to_owned(),
                        },
                        vec![Value::Object(Map::new())],
                    ),
                    (
                        Ability {
                            namespace: "namespace1".to_owned(),
                            name: "ability2".to_owned(),
                        },
                        vec![Value::Object(Map::new())],
                    ),
                ]),
            )]),
        };
        let statement = build_statement(&recap);
        assert_eq!(statement, expected_statement);
    }

    #[test]
    fn statement_1_uri_2_namespace_2_ability() {
        let expected_statement = "I further authorize the stated URI to perform the following actions on my behalf: (1) 'namespace1': 'ability1', 'ability2' for 'uri1'. (2) 'namespace2': 'ability1', 'ability2' for 'uri1'.";
        let recap = ReCapDetailsObject {
            att: HashMap::from([(
                "uri1".to_owned(),
                HashMap::from([
                    (
                        Ability {
                            namespace: "namespace1".to_owned(),
                            name: "ability1".to_owned(),
                        },
                        vec![Value::Object(Map::new())],
                    ),
                    (
                        Ability {
                            namespace: "namespace2".to_owned(),
                            name: "ability1".to_owned(),
                        },
                        vec![Value::Object(Map::new())],
                    ),
                    (
                        Ability {
                            namespace: "namespace1".to_owned(),
                            name: "ability2".to_owned(),
                        },
                        vec![Value::Object(Map::new())],
                    ),
                    (
                        Ability {
                            namespace: "namespace2".to_owned(),
                            name: "ability2".to_owned(),
                        },
                        vec![Value::Object(Map::new())],
                    ),
                ]),
            )]),
        };
        let statement = build_statement(&recap);
        assert_eq!(statement, expected_statement);
    }

    #[test]
    fn statement_2_uri_2_namespace_2_ability() {
        let expected_statement = "I further authorize the stated URI to perform the following actions on my behalf: (1) 'namespace1': 'ability1', 'ability2' for 'uri1'. (2) 'namespace2': 'ability1', 'ability2' for 'uri1'. (3) 'namespace1': 'ability1', 'ability2' for 'uri2'.";
        let recap = ReCapDetailsObject {
            att: HashMap::from([
                (
                    "uri2".to_owned(),
                    HashMap::from([
                        (
                            Ability {
                                namespace: "namespace1".to_owned(),
                                name: "ability1".to_owned(),
                            },
                            vec![Value::Object(Map::new())],
                        ),
                        (
                            Ability {
                                namespace: "namespace1".to_owned(),
                                name: "ability2".to_owned(),
                            },
                            vec![Value::Object(Map::new())],
                        ),
                    ]),
                ),
                (
                    "uri1".to_owned(),
                    HashMap::from([
                        (
                            Ability {
                                namespace: "namespace1".to_owned(),
                                name: "ability1".to_owned(),
                            },
                            vec![Value::Object(Map::new())],
                        ),
                        (
                            Ability {
                                namespace: "namespace2".to_owned(),
                                name: "ability1".to_owned(),
                            },
                            vec![Value::Object(Map::new())],
                        ),
                        (
                            Ability {
                                namespace: "namespace1".to_owned(),
                                name: "ability2".to_owned(),
                            },
                            vec![Value::Object(Map::new())],
                        ),
                        (
                            Ability {
                                namespace: "namespace2".to_owned(),
                                name: "ability2".to_owned(),
                            },
                            vec![Value::Object(Map::new())],
                        ),
                    ]),
                ),
            ]),
        };
        let statement = build_statement(&recap);
        assert_eq!(statement, expected_statement);
    }
}
