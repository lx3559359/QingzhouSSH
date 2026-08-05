mod command;
mod parser;
mod request;
mod result_store;

pub use command::build_search_command;
pub use parser::{parse_search_output, LogLineKind, LogMatch, RemoteFileMatch, SearchResultItem};
pub use request::{LogSearchRequest, LogSearchTarget};
pub use result_store::{LogResultPage, LogResultStore, StoredLogResults};
