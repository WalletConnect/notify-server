use {
    crate::error::{Error, Result},
    serde::{Deserialize, Serialize},
    uuid::Uuid,
};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct Notification {
    pub r#type: Uuid,
    pub title: String,
    pub body: String,
    pub icon: Option<String>,
    pub url: Option<String>,
}

impl Notification {
    pub fn validate(&self) -> Result<()> {
        if self.title.is_empty() {
            return Err(Error::BadRequest(
                "Notification title cannot be empty".into(),
            ));
        }
        if self.title.len() > 64 {
            return Err(Error::BadRequest(
                "Notification title cannot be longer than 64 characters".into(),
            ));
        }

        if self.body.is_empty() {
            return Err(Error::BadRequest(
                "Notification body cannot be empty".into(),
            ));
        }
        if self.body.len() > 255 {
            return Err(Error::BadRequest(
                "Notification body cannot be longer than 64 characters".into(),
            ));
        }

        if let Some(icon) = &self.icon {
            if icon.is_empty() {
                return Err(Error::BadRequest(
                    "Notification icon cannot be empty".into(),
                ));
            }
            if icon.len() > 255 {
                return Err(Error::BadRequest(
                    "Notification icon cannot be longer than 255 characters".into(),
                ));
            }
        }

        if let Some(url) = &self.url {
            if url.is_empty() {
                return Err(Error::BadRequest("Notification url cannot be empty".into()));
            }
            if url.len() > 255 {
                return Err(Error::BadRequest(
                    "Notification url cannot be longer than 255 characters".into(),
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

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
}
