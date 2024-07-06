use std::io::Write;
use anyhow::Result;
use std::path::PathBuf;
use directories::BaseDirs;
use tracing::{debug, info};
use serde_yml::ser::Serializer;
use serde::{Serialize, Deserialize};
use std::fs::File;
use std::io::{BufReader, BufWriter};

use crate::provider::Client;
use crate::patterns::Pattern;

pub struct SessionManager {
    pub store: PathBuf,
}

impl Default for SessionManager {
    fn default() -> Self {
        let store = BaseDirs::new()
            .map(|p| p.config_dir().join("fabric/sessions"))
            .unwrap_or(PathBuf::from("./sessions"));

        Self { store }
    }
}

impl SessionManager {
    pub fn list_sessions(&self) -> Result<Vec<String>> {
        let result = std::fs::read_dir(&self.store)?
            .filter_map(|d| d.ok())
            .map(|ent| ent.path())
            .filter(|p| p.extension().is_some_and(|x| x == "yml"))
            .filter_map(|p| p.file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string()));

        Ok(result.collect())
    }

    pub fn get_session<T: AsRef<str>>(&self, name: &Option<T>) -> Result<ChatSession> {
        match name {
            Some(n) => self.load_or_create(n.as_ref()),
            None => Ok(self.dummy_session()),
        }
    }

    pub fn dummy_session(&self) -> ChatSession {
        ChatSession::Dummy {
            messages: Vec::new(),
        }
    }

    pub fn load_or_create(&self, name: &str) -> Result<ChatSession> {
        let path = self.store.as_path()
            .join(name)
            .with_extension("yml");

        let current = self.load_session(name);

        match current {
            Ok(result) => Ok(result),
            Err(e) => {
                info!("Failed to load session {e:?}, creating new one");
                let file = File::create(&path)?;
                Ok(ChatSession::Stored {
                    file,
                    path,
                    messages: Vec::new(),
                })
            },
        }
    }

    pub fn load_session(&self, name: &str) -> Result<ChatSession> {
        let path = self.store.as_path()
            .join(name)
            .with_extension("yml");

        let file = File::options().read(true).append(true).open(&path)?;
        let reader = BufReader::new(&file);
        let messages: Vec<ChatEntry> = serde_yml::from_reader(reader)?;
        Ok(ChatSession::Stored { file, path, messages })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag="role", rename_all="lowercase")]
pub enum ChatEntry {
    #[serde(rename="user", alias="query")]
    Query {
        #[serde(skip_serializing_if="Option::is_none")]
        pattern: Option<String>,

        content: String,
    },

    #[serde(rename="assistant", alias="reply")]
    Reply {
        content: String,
    },

    #[serde(other)]
    Unknown,
}

impl ChatEntry {
    pub fn query<T: Into<String>, P: Into<String>>(content: T, pattern: Option<P>) -> Self {
        let content = content.into();
        let pattern = pattern.map(|p| p.into());

        Self::Query {
            pattern,
            content,
        }
    }

    pub fn user<T: Into<String>>(content: T) -> Self {
        Self::query(content, None as Option<String>)
    }

    pub fn reply<T: Into<String>>(content: T) -> Self {
        let content = content.into();

        Self::Reply {
            content
        }
    }

    pub fn assistant<T: Into<String>>(content: T) -> Self {
        let content = content.into();

        Self::Reply {
            content
        }
    }
}

#[derive(Debug)]
pub enum ChatSession {
    Stored {
        file: File,
        path: PathBuf,
        messages: Vec<ChatEntry>,
    },
    Dummy {
        messages: Vec<ChatEntry>,
    },
}

impl ChatSession {
    pub fn is_dummy(&self) -> bool {
        match self {
            ChatSession::Dummy { .. } => true,
            _ => false,
        }
    }

    pub fn messages(&self) -> &[ChatEntry] {
        match self {
            ChatSession::Stored { messages, .. } => messages,
            ChatSession::Dummy { messages, .. } => messages,
        }
    }

    pub fn mut_messages(&mut self) -> &mut Vec<ChatEntry> {
        match self {
            ChatSession::Stored { messages, .. } => messages,
            ChatSession::Dummy { messages, .. } => messages,
        }
    }

    pub fn append(&mut self, entry: ChatEntry) -> Result<()> {
        match self {
            ChatSession::Stored { file, .. } => {
                let mut buf = BufWriter::new(file);
                let mut ser = Serializer::new(&mut buf);
                [&entry].serialize(&mut ser)?;
            },
            _ => {},
        };

        let messages = self.mut_messages();

        messages.append(&mut vec![entry]); // Hmm, maybe just move it

        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        match self {
            ChatSession::Stored {path , ..} => std::fs::remove_file(path)?,
            _ => {},
        };
        Ok(())
    }

    pub fn prune(&mut self, limit: usize) -> Result<Vec<ChatEntry>> {
        match self {
            ChatSession::Stored {path , messages, ..} => {
                let len = messages.len().min(limit);
                let start = messages.len() -len;
                let discard = messages.drain(..start).collect::<Vec<_>>();
                debug!("Discarding {} entries", discard.len());

                let file = File::open(path)?;
                let mut writer = BufWriter::new(file);
                let mut ser = Serializer::new(&mut writer);
                messages.serialize(&mut ser)?;

                Ok(discard)
            },
            _ => Ok(vec![]),
        }
    }

    pub fn with_client(self, client: Box<dyn Client>) -> SessionWithClient {
        SessionWithClient {
            inner: self,
            client,
        }
    }
}

pub struct SessionWithClient {
    pub inner: ChatSession,
    pub client: Box<dyn Client>,

}

impl SessionWithClient {
    pub fn is_dummy(&self) -> bool {
        self.inner.is_dummy()
    }

    pub fn messages(&self) -> &[ChatEntry] {
        self.inner.messages()
    }

    pub fn mut_messages(&mut self) -> &mut Vec<ChatEntry> {
        self.inner.mut_messages()
    }

    pub async fn send_message<S: AsRef<str>, W: Write>(&mut self, pattern: &Pattern, text: S, out: &mut W) -> Result<()> {
        self.inner.append(ChatEntry::query(text.as_ref(), Some(&pattern.name)))?;
        let result = self.client.send_message(&pattern, &self.inner).await?;
        info!("Message metadata {:?}", result.meta);

        writeln!(out, "{}", &result.body)?;

        self.inner.append(ChatEntry::assistant(&result.body))?;
        Ok(())
    }

    pub async fn stream_message<S: AsRef<str>, W: Write>(&mut self, pattern: &Pattern, text: S, out: &mut W) -> Result<()> {
        let session = &mut self.inner;
        let client = &mut self.client;

        session.append(ChatEntry::query(text.as_ref(), Some(&pattern.name)))?;
        let result = client.stream_message(&pattern, &session).await?;
        info!("Message metadata {:?}", result.meta);

        let mut rx = result.rx;

        let mut content = if session.is_dummy() { None } else { Some(String::new()) };

        while let Some(Ok(msg)) = rx.recv().await {
            write!(out, "{}", &msg)?;
            out.flush()?;

            if let Some(content) = content.as_mut() {content.push_str(&msg)};
        }

        if let Some(content) = content {
            session.append(ChatEntry::assistant(&content))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use cool_asserts::assert_matches;
    use indoc::indoc;
    use serde_yml::from_str;

    use super::*;

    #[test]
    fn get_session_without_name() -> Result<()> {
        let manager = SessionManager::default();
        let name: Option<String> = None;
        let result = manager.get_session(&name)?;

        assert!(result.is_dummy());
        assert_matches!(result, ChatSession::Dummy {..});

        Ok(())
    }

    #[test]
    fn load_user_entry_pass() -> Result<()> {
        let input = indoc! {r#"
            - role: user
              content: |
                Hello, and a good day to you sir!
        "#};

        let result: Vec<ChatEntry> = from_str(input)?;

        assert_eq!(result.len(), 1);
        assert_matches!(result[0], ChatEntry::Query {..});

        Ok(())
    }

    #[test]
    fn load_unknown_entry_ignored() -> Result<()> {
        let input = indoc! {r#"
            - role: cow
              content: |
                Moo? Moo!
        "#};

        let result: Vec<ChatEntry> = from_str(input)?;

        assert_eq!(result.len(), 1);
        assert_matches!(result[0], ChatEntry::Unknown);

        Ok(())
    }
}
