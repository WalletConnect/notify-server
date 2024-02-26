use {
    crate::error::NotifyServerError,
    serde::{Deserialize, Serialize},
    uuid::Uuid,
    validator::Validate,
};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Validate)]
pub struct Notification {
    pub r#type: Uuid,
    #[validate(length(min = 1, max = 64))]
    pub title: String,
    #[validate(length(min = 1, max = 255))]
    pub body: String,
    #[validate(length(min = 1, max = 255))]
    pub icon: Option<String>,
    #[validate(length(min = 1, max = 255))]
    pub url: Option<String>,
}

impl Notification {
    pub fn validate(&self) -> Result<(), NotifyServerError> {
        Validate::validate(&self)
            .map_err(|error| NotifyServerError::UnprocessableEntity(error.to_string()))
    }
}

#[cfg(test)]
mod test {
    use {super::*, serde_json::json};

    #[test]
    fn valid_notification() {
        assert!(Notification {
            r#type: Uuid::new_v4(),
            title: "title".to_owned(),
            body: "body".to_owned(),
            icon: None,
            url: None,
        }
        .validate()
        .is_ok());

        assert!(Notification {
            r#type: Uuid::new_v4(),
            title: "title".to_owned(),
            body: "body".to_owned(),
            icon: Some("icon".to_owned()),
            url: Some("url".to_owned()),
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn invalid_notification() {
        assert!(Notification {
            r#type: Uuid::new_v4(),
            title: "".to_owned(),
            body: "body".to_owned(),
            icon: None,
            url: None,
        }
        .validate()
        .is_err());

        assert!(Notification {
            r#type: Uuid::new_v4(),
            title: "title".to_owned(),
            body: "".to_owned(),
            icon: Some("icon".to_owned()),
            url: Some("url".to_owned()),
        }
        .validate()
        .is_err());

        assert!(Notification {
            r#type: Uuid::new_v4(),
            title: "title".to_owned(),
            body: "body".to_owned(),
            icon: Some("".to_owned()),
            url: Some("url".to_owned()),
        }
        .validate()
        .is_err());

        assert!(Notification {
            r#type: Uuid::new_v4(),
            title: "title".to_owned(),
            body: "body".to_owned(),
            icon: Some("icon".to_owned()),
            url: Some("".to_owned()),
        }
        .validate()
        .is_err());
    }

    #[test]
    fn optional_fields() {
        serde_json::from_value::<Notification>(json!({
            "type": Uuid::new_v4(),
            "title": "title",
            "body": "body"
        }))
        .unwrap();
    }
}
