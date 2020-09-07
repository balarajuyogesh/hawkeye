use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::HashMap;

#[skip_serializing_none]
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Watcher {
    pub id: Option<String>,
    pub description: Option<String>,
    pub slate_url: String,
    pub status: Status,
    pub source: Source,
    pub transitions: Vec<Transition>,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Ready,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Source {
    pub ingest_port: u32,
    pub container: Container,
    pub codec: Codec,
    pub transport: Protocol,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Container {
    MpegTs,
    Fmp4,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    H264,
    H265,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug, Eq, PartialEq)]
#[serde(tag = "protocol", rename_all = "lowercase")]
pub enum Protocol {
    Rtp,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct Transition {
    pub from: VideoMode,
    pub to: VideoMode,
    pub actions: Vec<Action>,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum VideoMode {
    Slate,
    Content,
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    HttpCall(HttpCall),

    #[cfg(test)]
    #[serde(skip_serializing, skip_deserializing)]
    FakeAction(FakeAction),
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FakeAction {
    pub(crate) called:  std::rc::Rc<std::cell::Cell<bool>>,
    pub(crate) execute_returns: Option<Result<(), ()>>,
}

#[cfg(test)]
impl FakeAction {
    pub(crate) fn execute(&mut self) -> color_eyre::Result<()> {
        self.called.set(true);
        if let Some(result) = self.execute_returns.take() {
            match result {
                Ok(()) => Ok(()),
                Err(_) => Err(color_eyre::Report::msg("Err")),
            }
        } else {
            Err(color_eyre::Report::msg("Err"))
        }
    }
}

#[skip_serializing_none]
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct HttpCall {
    pub(crate) method: HttpMethod,
    pub(crate) url: String,
    pub(crate) description: Option<String>,
    pub(crate) authorization: Option<HttpAuth>,
    pub(crate) headers: Option<HashMap<String, String>>,
    pub(crate) body: Option<String>,
    pub(crate) retries: Option<u8>,
    pub(crate) timeout: Option<u32>,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug, Eq, PartialEq)]
pub enum HttpMethod {
    POST,
    GET,
    PUT,
    PATCH,
    DELETE,
}

impl ToString for HttpMethod {
    fn to_string(&self) -> String {
        match self {
            HttpMethod::POST => "POST".to_string(),
            HttpMethod::GET => "GET".to_string(),
            HttpMethod::PUT => "PUT".to_string(),
            HttpMethod::PATCH => "PATCH".to_string(),
            HttpMethod::DELETE => "DELETE".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HttpAuth {
    Basic { username: String, password: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs::File;
    use std::io::Read;

    fn get_watcher() -> Watcher {
        Watcher {
            id: Some("ee21fc9a-7225-450b-a2a7-2faf914e35b8".to_string()),
            description: Some("UEFA 2020 - Lyon vs. Bayern".to_string()),
            slate_url: "http://thumbor.cbs.com/orignal/hawkeye/video-slate.jpg".to_string(),
            status: Status::Running,
            source: Source {
                ingest_port: 5000,
                container: Container::MpegTs,
                codec: Codec::H264,
                transport: Protocol::Rtp
            },
            transitions: vec![
                Transition {
                    from: VideoMode::Content,
                    to: VideoMode::Slate,
                    actions: vec![
                        Action::HttpCall( HttpCall {
                            description: Some("Trigger AdBreak using API".to_string()),
                            method: HttpMethod::POST,
                            url: "http://non-existent.cbs.com/v1/organization/cbsa/channel/slate4/ad-break".to_string(),
                            authorization: Some(HttpAuth::Basic {
                                username: "dev_user".to_string(),
                                password: "something".to_string()
                            }),
                            headers: Some([("Content-Type", "application/json")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<HashMap<String, String>>()),
                            body: Some("{\"duration\":300}".to_string()),
                            retries: Some(3),
                            timeout: Some(10),
                        })
                    ]
                },
                Transition {
                    from: VideoMode::Slate,
                    to: VideoMode::Content,
                    actions: vec![
                        Action::HttpCall( HttpCall {
                            description: Some("Use dump out of AdBreak API call".to_string()),
                            method: HttpMethod::DELETE,
                            url: "http://non-existent.cbs.com/v1/organization/cbsa/channel/slate4/ad-break".to_string(),
                            authorization: Some(HttpAuth::Basic {
                                username: "dev_user".to_string(),
                                password: "something".to_string()
                            }),
                            headers: None,
                            body: None,
                            retries: None,
                            timeout: Some(10),
                        })
                    ]
                }
            ]
        }
    }

    #[test]
    fn deserialize_as_expected() {
        let mut fixture = File::open("fixtures/watcher.json").expect("Fixture was not found!");
        let mut expected_value = String::new();
        fixture.read_to_string(&mut expected_value).unwrap();
        let expected: Watcher = serde_json::from_str(expected_value.as_str()).unwrap();

        assert_eq!(get_watcher(), expected);
    }

    #[test]
    fn serialize_as_expected() {
        let mut fixture = File::open("fixtures/watcher.json").expect("Fixture was not found!");
        let mut expected_value = String::new();
        fixture.read_to_string(&mut expected_value).unwrap();
        let fixture: serde_json::Value = serde_json::from_str(expected_value.as_str()).unwrap();

        let watcher = get_watcher();
        let watcher_json = serde_json::to_string(&watcher).unwrap();
        let watcher_as_value: serde_json::Value =
            serde_json::from_str(watcher_json.as_str()).unwrap();

        assert_eq!(watcher_as_value, fixture);
    }
}
