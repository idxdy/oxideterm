use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRedisStatus {
    #[default]
    Unknown,
    Available,
    /// The host has no `redis-cli` binary and no Docker container to inspect.
    /// Containerized or server-only installs have no client.
    Unavailable,
    /// Nothing is reachable for the probed endpoint.
    NoServer,
    /// The server answers but requires authentication, or the supplied
    /// password was rejected.
    AuthRequired,
    Error {
        message: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRedisEndpointKind {
    /// A server listening on the host's own loopback, read through the host's
    /// `redis-cli`.
    Host,
    /// A container read through `docker exec`, which works without a published
    /// port and without a client on the host.
    Docker,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRedisEndpoint {
    pub kind: ResourceRedisEndpointKind,
    /// `127.0.0.1:6379` for host endpoints, the container name for Docker ones.
    pub target: String,
}

impl ResourceRedisEndpoint {
    fn new(kind: ResourceRedisEndpointKind, target: impl Into<String>) -> Self {
        Self {
            kind,
            target: target.into(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRedisSnapshot {
    pub status: ResourceRedisStatus,
    pub endpoints: Vec<ResourceRedisEndpoint>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRedisDatabase {
    pub index: u32,
    /// Keys the server reports for this database, which can exceed the number
    /// of keys returned by the bounded scan.
    pub key_count: u64,
    pub keys: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRedisKeys {
    pub status: ResourceRedisStatus,
    pub databases: Vec<ResourceRedisDatabase>,
}

/// One node of the colon-separated key tree.
///
/// `path` is the full colon-joined prefix, so the UI can key expansion state by
/// path and two sibling databases cannot collide.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceRedisKeyNode {
    pub label: String,
    pub path: String,
    /// The key with exactly this path also exists, not just its descendants.
    pub is_key: bool,
    /// Keys in this subtree, including `is_key` itself.
    pub key_count: usize,
    pub children: Vec<ResourceRedisKeyNode>,
}

/// Keys returned per database. A single `SCAN` pass is bounded so a production
/// database with millions of keys cannot stall the capture or the panel.
pub const REDIS_KEYS_PER_DATABASE_LIMIT: usize = 200;

const REDIS_UNAVAILABLE_MARKER: &str = "__OXIDE_REDIS_UNAVAILABLE__";
const REDIS_ERROR_MARKER: &str = "__OXIDE_REDIS_ERROR__";
const REDIS_OK_LINE: &str = "REDIS_OK";
pub const REDIS_PROBE_TARGET: &str = "127.0.0.1:6379";

/// Sentinel the create script prints when the key already exists; the panel
/// replaces it with a localized message.
pub const REDIS_KEY_EXISTS_SENTINEL: &str = "__OXIDE_KEY_EXISTS__";

const REDIS_KEYS_SECTION: &str = "REDISKEYS";
const REDIS_VALUE_SECTION: &str = "REDISVALUE";
const REDIS_ACTION_SECTION: &str = "REDISACTION";
/// The raw value reply starts after this marker so its lines cannot be parsed
/// as metadata.
const REDIS_BODY_MARKER: &str = "===REDIS_BODY===";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRedisValueKind {
    #[default]
    Other,
    String,
    /// RedisJSON keys report `ReJSON-RL`/`json`; the body is the JSON document.
    Json,
    Hash,
    List,
    Set,
    SortedSet,
    Stream,
}

impl ResourceRedisValueKind {
    fn from_type_reply(reply: &str) -> Self {
        match reply {
            "string" => Self::String,
            "hash" => Self::Hash,
            "list" => Self::List,
            "set" => Self::Set,
            "zset" => Self::SortedSet,
            "stream" => Self::Stream,
            "json" => Self::Json,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRedisTtl {
    #[default]
    Unknown,
    /// The key has no expiration (`TTL` = -1).
    Never,
    /// The key vanished between listing and browsing (`TTL` = -2).
    Missing,
    Seconds(u64),
}

impl ResourceRedisValueKind {
    /// Raw Redis type name for display and for "unsupported type" messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Json => "json",
            Self::Hash => "hash",
            Self::List => "list",
            Self::Set => "set",
            Self::SortedSet => "zset",
            Self::Stream => "stream",
            Self::Other => "other",
        }
    }
}

/// One key's metadata and value, read through `redis-cli`'s plain replies.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRedisValue {
    pub status: ResourceRedisStatus,
    pub kind: ResourceRedisValueKind,
    /// Raw `TYPE` reply for module types (`Timeseries`, `ReJSON-RL`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_type: Option<String>,
    pub ttl: ResourceRedisTtl,
    /// `MEMORY USAGE` bytes when the server supports it.
    pub memory_usage: Option<u64>,
    /// String bytes, or element count for the collection types.
    pub length: Option<u64>,
    pub body: String,
}

// Windows hosts keep the availability probe: key browsing drives `docker exec`
// and a POSIX `while read` loop, which a PowerShell host cannot share.
const REDIS_PROBE_COMMAND_WINDOWS: &str = concat!(
    "Write-Output '===REDIS===';",
    "if(Get-Command redis-cli -ErrorAction SilentlyContinue){",
    "$oxideRedisReply=& redis-cli -h 127.0.0.1 -p 6379 -t 3 PING 2>&1;",
    "if(\"$oxideRedisReply\" -eq 'PONG'){Write-Output 'REDIS_OK'}",
    "else{",
    "$oxideRedisFirst=($oxideRedisReply|Select-Object -First 1) -replace [char]9,' ' -replace [char]13,'';",
    "Write-Output ('__OXIDE_REDIS_ERROR__'+[char]9+$oxideRedisFirst)",
    "}",
    "}else{Write-Output '__OXIDE_REDIS_UNAVAILABLE__'};",
    "Write-Output '===REDIS_END===';"
);

// Discovery never sends credentials: it lists the endpoints a follow-up browse
// can read. Docker and Podman share `ps --format` and `exec`, so one loop covers
// both runtimes. A container that publishes host port 6379 owns the loopback
// address, so the host endpoint is suppressed to avoid listing the same server
// twice.
const REDIS_DISCOVERY_COMMAND_UNIX: &str = concat!(
    "echo '===REDIS==='; ",
    "if command -v redis-cli >/dev/null 2>&1; then ",
    "printf 'client\\tpresent\\n'; ",
    "else printf 'client\\tmissing\\n'; fi; ",
    "for oxide_runtime in docker podman; do ",
    "if command -v $oxide_runtime >/dev/null 2>&1; then ",
    "$oxide_runtime ps --format '{{.Names}}\t{{.Image}}\t{{.Ports}}' 2>/dev/null | ",
    "while IFS='\t' read -r oxide_name oxide_image oxide_ports; do ",
    "case \"$oxide_image\" in ",
    "*redis*|*Redis*|*REDIS*|*valkey*|*Valkey*|*keydb*|*KeyDB*) ",
    "printf 'endpoint\\tcontainer\\t%s\\n' \"$oxide_name\"; ",
    "case \"$oxide_ports\" in *':6379->'*) printf 'published\\t%s\\n' \"$oxide_name\";; esac ",
    ";; esac; done; fi; done; ",
    "echo '===REDIS_END==='"
);

// Commands read their arguments from the exec channel's stdin: the password and
// the key itself never become part of a command line (visible to `ps`) or the
// remote shell history. The password is only ever exported as `REDISCLI_AUTH`
// for the child process; `-a`/`--askpass` are never used.
const REDIS_HOST_CLI_FUNCTION: &str = concat!(
    "oxide_cli() { ",
    "if [ -n \"$oxide_pw\" ]; then REDISCLI_AUTH=$oxide_pw redis-cli -t 5 \"$@\"; ",
    "else redis-cli -t 5 \"$@\"; fi; ",
    "}; ",
    // A host process inherits the caller's pipe directly, so streaming writes
    // share the same helper as the plain calls.
    "oxide_cli_stdin() { oxide_cli \"$@\"; }; "
);

const REDIS_CONTAINER_CLI_FUNCTION: &str = concat!(
    "oxide_runtime=docker; ",
    "if ! command -v docker >/dev/null 2>&1; then oxide_runtime=podman; fi; ",
    "oxide_cli() { ",
    "if [ -n \"$oxide_pw\" ]; then ",
    "REDISCLI_AUTH=$oxide_pw $oxide_runtime exec -e REDISCLI_AUTH {container} redis-cli -t 5 \"$@\"; ",
    "else $oxide_runtime exec {container} redis-cli -t 5 \"$@\"; fi; ",
    "}; ",
    // `redis-cli -x` reads the value from stdin, and `podman/docker exec` only
    // forwards stdin with `-i`. Only the streaming writes use this variant, and
    // each one is fed by its own pipe, so the script's stdin stays untouched.
    "oxide_cli_stdin() { ",
    "if [ -n \"$oxide_pw\" ]; then ",
    "REDISCLI_AUTH=$oxide_pw $oxide_runtime exec -i -e REDISCLI_AUTH {container} redis-cli -t 5 \"$@\"; ",
    "else $oxide_runtime exec -i {container} redis-cli -t 5 \"$@\"; fi; ",
    "}; "
);

// `INFO keyspace` only lists databases that hold keys, so the empty ones are
// synthesized from `CONFIG GET databases` for the "all databases" rail view.
const REDIS_KEYS_TAIL: &str = concat!(
    "oxide_reply=$(oxide_cli PING 2>&1); ",
    "case \"$oxide_reply\" in ",
    "*PONG*) ",
    "printf 'status\\tready\\n'; ",
    "oxide_dbs=16; ",
    "oxide_cfg=$(oxide_cli CONFIG GET databases 2>/dev/null | tail -n 1 | tr -d '\\r'); ",
    "case \"$oxide_cfg\" in ''|*[!0-9]*) ;; *) oxide_dbs=$oxide_cfg;; esac; ",
    "oxide_info=$(oxide_cli INFO keyspace 2>/dev/null | tr -d '\\r'); ",
    "oxide_i=0; ",
    "while [ \"$oxide_i\" -lt \"$oxide_dbs\" ]; do ",
    "oxide_line=$(printf '%s\\n' \"$oxide_info\" | grep \"^db${oxide_i}:keys=\" | head -n 1); ",
    "oxide_keys=0; ",
    "if [ -n \"$oxide_line\" ]; then oxide_keys=$(printf '%s' \"$oxide_line\" | ",
    "sed -n 's/^db[0-9][0-9]*:keys=\\([0-9][0-9]*\\).*/\\1/p'); fi; ",
    "case \"$oxide_keys\" in ''|*[!0-9]*) oxide_keys=0;; esac; ",
    "printf 'db\\t%s\\t%s\\n' \"$oxide_i\" \"$oxide_keys\"; ",
    "if [ \"$oxide_keys\" -gt 0 ]; then ",
    "oxide_cli -n \"$oxide_i\" --scan --count 512 2>/dev/null | head -n OOLIMIT | ",
    "while IFS= read -r oxide_key; do printf 'key\\t%s\\t%s\\n' \"$oxide_i\" \"$oxide_key\"; done; fi; ",
    "oxide_i=$((oxide_i + 1)); ",
    "done ",
    ";; ",
    "*NOAUTH*|*WRONGPASS*|*'invalid password'*|*'no password is set'*|*'AUTH failed'*) ",
    "printf 'status\\tauth\\n' ;; ",
    "*'Connection refused'*|*'Could not connect'*) printf 'status\\tnoserver\\n' ;; ",
    "*) printf 'status\\terror\\t%s\\n' ",
    "\"$(printf '%s' \"$oxide_reply\" | head -n 1 | tr '\\t' ' ' | tr -d '\\r')\" ;; ",
    "esac; "
);

const REDIS_VALUE_TAIL: &str = concat!(
    "case \"$oxide_db\" in ''|*[!0-9]*) oxide_db=0;; esac; ",
    "if [ -z \"$oxide_key\" ]; then printf 'status\\tmissing\\n'; else ",
    "oxide_type=$(oxide_cli -n \"$oxide_db\" TYPE \"$oxide_key\" 2>&1 | head -n 1 | tr -d '\\r'); ",
    "case \"$oxide_type\" in ",
    "none) printf 'status\\tmissing\\n' ;; ",
    "string|hash|list|set|zset|stream) ",
    "printf 'status\\tready\\n'; ",
    "printf 'type\\t%s\\n' \"$oxide_type\"; ",
    "printf 'ttl\\t%s\\n' \"$(oxide_cli -n \"$oxide_db\" TTL \"$oxide_key\" 2>/dev/null | head -n 1 | tr -d '\\r')\"; ",
    "printf 'usage\\t%s\\n' \"$(oxide_cli -n \"$oxide_db\" MEMORY USAGE \"$oxide_key\" 2>/dev/null | head -n 1 | tr -d '\\r')\"; ",
    "oxide_body=''; ",
    "case \"$oxide_type\" in ",
    "string) printf 'length\\t%s\\n' \"$(oxide_cli -n \"$oxide_db\" STRLEN \"$oxide_key\" 2>/dev/null | head -n 1 | tr -d '\\r')\"; ",
    "oxide_body=$(oxide_cli -n \"$oxide_db\" GET \"$oxide_key\" 2>/dev/null) ;; ",
    "hash) printf 'length\\t%s\\n' \"$(oxide_cli -n \"$oxide_db\" HLEN \"$oxide_key\" 2>/dev/null | head -n 1 | tr -d '\\r')\"; ",
    "oxide_body=$(oxide_cli -n \"$oxide_db\" HGETALL \"$oxide_key\" 2>/dev/null | head -n 400) ;; ",
    "list) printf 'length\\t%s\\n' \"$(oxide_cli -n \"$oxide_db\" LLEN \"$oxide_key\" 2>/dev/null | head -n 1 | tr -d '\\r')\"; ",
    "oxide_body=$(oxide_cli -n \"$oxide_db\" LRANGE \"$oxide_key\" 0 199 2>/dev/null) ;; ",
    "set) printf 'length\\t%s\\n' \"$(oxide_cli -n \"$oxide_db\" SCARD \"$oxide_key\" 2>/dev/null | head -n 1 | tr -d '\\r')\"; ",
    "oxide_body=$(oxide_cli -n \"$oxide_db\" SMEMBERS \"$oxide_key\" 2>/dev/null | head -n OOLIMIT) ;; ",
    "zset) printf 'length\\t%s\\n' \"$(oxide_cli -n \"$oxide_db\" ZCARD \"$oxide_key\" 2>/dev/null | head -n 1 | tr -d '\\r')\"; ",
    "oxide_body=$(oxide_cli -n \"$oxide_db\" ZRANGE \"$oxide_key\" 0 199 WITHSCORES 2>/dev/null) ;; ",
    "stream) printf 'length\\t%s\\n' \"$(oxide_cli -n \"$oxide_db\" XLEN \"$oxide_key\" 2>/dev/null | head -n 1 | tr -d '\\r')\" ;; ",
    "esac; ",
    "printf '===REDIS_BODY===\\n%s\\n' \"$oxide_body\"; ",
    ";; ",
    "ReJSON-RL|json) ",
    "printf 'status\\tready\\n'; ",
    "printf 'type\\tjson\\n'; ",
    "printf 'ttl\\t%s\\n' \"$(oxide_cli -n \"$oxide_db\" TTL \"$oxide_key\" 2>/dev/null | head -n 1 | tr -d '\\r')\"; ",
    "printf 'usage\\t%s\\n' \"$(oxide_cli -n \"$oxide_db\" MEMORY USAGE \"$oxide_key\" 2>/dev/null | head -n 1 | tr -d '\\r')\"; ",
    "printf '===REDIS_BODY===\\n%s\\n' \"$(oxide_cli -n \"$oxide_db\" JSON.GET \"$oxide_key\" 2>/dev/null)\"; ",
    ";; ",
    "*) ",
    "printf 'status\\tready\\n'; ",
    "printf 'type\\tother\\n'; ",
    "printf 'rawtype\\t%s\\n' \"$(printf '%s' \"$oxide_type\" | tr '\\t' ' ')\" ;; ",
    "printf '===REDIS_BODY===\\n%s\\n' \"$oxide_body\"; ",
    ";; ",
    "*NOAUTH*|*WRONGPASS*|*'invalid password'*|*'no password is set'*|*'AUTH failed'*) ",
    "printf 'status\\tauth\\n' ;; ",
    "*) printf 'status\\terror\\t%s\\n' \"$(printf '%s' \"$oxide_type\" | tr '\\t' ' ')\" ;; ",
    "esac; fi; "
);

/// Value shapes the create dialog can write in one shot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedisCreateKind {
    #[default]
    String,
    /// Stored as a JSON document but written like a string; the panel validates it.
    Json,
    Hash,
    List,
    Set,
    SortedSet,
}

impl RedisCreateKind {
    /// Body layout the script expects: one element per line, tab-separated
    /// where a second field is needed.
    fn write_script(self) -> &'static str {
        match self {
            Self::String | Self::Json => concat!(
                "oxide_out=$(printf '%s' \"$oxide_body\" | oxide_cli_stdin -n \"$oxide_db\" -x SET \"$oxide_key\" 2>&1); ",
            ),
            Self::Hash => concat!(
                "oxide_out=$(printf '%s\\n' \"$oxide_body\" | ",
                "while IFS='\t' read -r oxide_field oxide_field_value; do ",
                "if [ -z \"$oxide_field\" ]; then continue; fi; ",
                "printf '%s' \"$oxide_field_value\" | oxide_cli_stdin -n \"$oxide_db\" -x HSET \"$oxide_key\" \"$oxide_field\"; echo; ",
                "done); ",
            ),
            Self::List => concat!(
                "oxide_out=$(printf '%s\\n' \"$oxide_body\" | ",
                "while IFS= read -r oxide_item; do ",
                "if [ -z \"$oxide_item\" ]; then continue; fi; ",
                "printf '%s' \"$oxide_item\" | oxide_cli_stdin -n \"$oxide_db\" -x RPUSH \"$oxide_key\"; echo; ",
                "done); ",
            ),
            Self::Set => concat!(
                "oxide_out=$(printf '%s\\n' \"$oxide_body\" | ",
                "while IFS= read -r oxide_item; do ",
                "if [ -z \"$oxide_item\" ]; then continue; fi; ",
                "printf '%s' \"$oxide_item\" | oxide_cli_stdin -n \"$oxide_db\" -x SADD \"$oxide_key\"; echo; ",
                "done); ",
            ),
            Self::SortedSet => concat!(
                "oxide_out=$(printf '%s\\n' \"$oxide_body\" | ",
                "while IFS='\t' read -r oxide_member oxide_score; do ",
                "if [ -z \"$oxide_member\" ]; then continue; fi; ",
                "printf '%s' \"$oxide_member\" | oxide_cli_stdin -n \"$oxide_db\" -x ZADD \"$oxide_key\" \"$oxide_score\"; echo; ",
                "done); ",
            ),
        }
    }
}

/// Guards the create flow: an existing key is never overwritten, and a failed
/// element write aborts before the expiry is applied.
const REDIS_CREATE_KEY_TEMPLATE: &str = concat!(
    "case \"$oxide_db\" in ''|*[!0-9]*) oxide_db=0;; esac; ",
    "oxide_body=$(cat); ",
    "if [ -z \"$oxide_key\" ]; then printf 'status\\terror\\tmissing key\\n'; else ",
    "oxide_exists=$(oxide_cli -n \"$oxide_db\" EXISTS \"$oxide_key\" 2>/dev/null | head -n 1 | tr -d '\\r'); ",
    "if [ \"$oxide_exists\" = \"1\" ]; then printf 'status\\terror\\t__OXIDE_KEY_EXISTS__\\n'; else ",
    "oxide_ok=1; oxide_message=''; ",
    "{WRITE}",
    "case \"$oxide_out\" in ",
    "*ERR*|*WRONGTYPE*|*NOAUTH*|*WRONGPASS*|*'not an integer'*) ",
    "oxide_ok=0; ",
    "oxide_message=$(printf '%s\\n' \"$oxide_out\" | grep -E 'ERR|WRONGTYPE|NOAUTH|WRONGPASS|not an integer' | head -n 1 | tr '\\t' ' ' | tr -d '\\r'); ",
    "if [ -z \"$oxide_message\" ]; then ",
    "oxide_message=$(printf '%s' \"$oxide_out\" | head -n 1 | tr '\\t' ' ' | tr -d '\\r'); fi; ",
    "esac; ",
    "if [ \"$oxide_ok\" = \"1\" ] && [ \"{TTL}\" -gt 0 ]; then ",
    "oxide_cli -n \"$oxide_db\" EXPIRE \"$oxide_key\" \"{TTL}\" >/dev/null 2>&1; fi; ",
    "if [ \"$oxide_ok\" = \"1\" ]; then printf 'status\\tok\\n'; ",
    "else printf 'status\\terror\\t%s\\n' \"$oxide_message\"; fi; ",
    "fi; fi; "
);

const REDIS_ACTION_TAIL_DELETE: &str = concat!(
    "case \"$oxide_db\" in ''|*[!0-9]*) oxide_db=0;; esac; ",
    "if [ -z \"$oxide_key\" ]; then printf 'status\\terror\\tmissing key\\n'; else ",
    "oxide_reply=$(oxide_cli -n \"$oxide_db\" DEL \"$oxide_key\" 2>&1 | head -n 1 | tr -d '\\r'); ",
    "case \"$oxide_reply\" in ",
    "'') printf 'status\\terror\\tempty reply\\n' ;; ",
    "*[!0-9]*) printf 'status\\terror\\t%s\\n' \"$(printf '%s' \"$oxide_reply\" | tr '\\t' ' ')\" ;; ",
    "*) printf 'status\\tok\\n' ;; ",
    "esac; fi; "
);

const REDIS_ACTION_TAIL_RENAME: &str = concat!(
    "case \"$oxide_db\" in ''|*[!0-9]*) oxide_db=0;; esac; ",
    "if [ -z \"$oxide_key\" ] || [ -z \"$oxide_new\" ] || [ \"$oxide_new\" = \"$oxide_key\" ]; then ",
    "printf 'status\\terror\\tunchanged key name\\n'; else ",
    "oxide_reply=$(oxide_cli -n \"$oxide_db\" RENAME \"$oxide_key\" \"$oxide_new\" 2>&1 | head -n 1 | tr -d '\\r'); ",
    "case \"$oxide_reply\" in ",
    "OK) printf 'status\\tok\\n' ;; ",
    "*) printf 'status\\terror\\t%s\\n' \"$(printf '%s' \"$oxide_reply\" | tr '\\t' ' ')\" ;; ",
    "esac; fi; "
);

/// Redis key mutations the panel can issue against one key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RedisKeyMutation {
    Delete,
    Rename,
}

pub fn redis_discovery_command(os_type: &str) -> &'static str {
    match os_type {
        "Windows" | "windows" => REDIS_PROBE_COMMAND_WINDOWS,
        _ => REDIS_DISCOVERY_COMMAND_UNIX,
    }
}

/// Builds the `oxide_cli` shell function for one endpoint.
///
/// Container names come from `docker ps`/`podman ps`, so they are re-validated
/// before interpolation instead of being trusted as shell text.
fn redis_cli_function(endpoint: &ResourceRedisEndpoint) -> Option<String> {
    match endpoint.kind {
        ResourceRedisEndpointKind::Host => Some(REDIS_HOST_CLI_FUNCTION.to_string()),
        ResourceRedisEndpointKind::Docker => {
            if !is_valid_container_name(&endpoint.target) {
                return None;
            }
            Some(REDIS_CONTAINER_CLI_FUNCTION.replace("{container}", &endpoint.target))
        }
    }
}

/// Assembles a POSIX script whose arguments arrive on stdin.
fn redis_script(section: &str, reads: &[&str], cli: &str, tail: &str) -> String {
    let mut script = format!("echo '==={section}==='; ");
    for variable in reads {
        script.push_str(&format!("IFS= read -r {variable} || {variable}=''; "));
    }
    script.push_str(cli);
    script.push_str(&tail.replace("OOLIMIT", &REDIS_KEYS_PER_DATABASE_LIMIT.to_string()));
    script.push_str(&format!("echo '==={section}_END==='"));
    script
}

fn redis_script_for(
    endpoint: &ResourceRedisEndpoint,
    os_type: &str,
    section: &str,
    reads: &[&str],
    tail: &str,
) -> Option<String> {
    if matches!(os_type, "Windows" | "windows") {
        return None;
    }
    let cli = redis_cli_function(endpoint)?;
    Some(redis_script(section, reads, &cli, tail))
}

/// Builds the key-browsing command for one endpoint.
///
/// Returns `None` when the host shell cannot run the POSIX script.
pub fn redis_keys_command(endpoint: &ResourceRedisEndpoint, os_type: &str) -> Option<String> {
    redis_script_for(
        endpoint,
        os_type,
        REDIS_KEYS_SECTION,
        &["oxide_pw"],
        REDIS_KEYS_TAIL,
    )
}

/// Builds the value-read command for one key.
///
/// stdin carries the password, then the database index, then the key.
pub fn redis_value_command(endpoint: &ResourceRedisEndpoint, os_type: &str) -> Option<String> {
    redis_script_for(
        endpoint,
        os_type,
        REDIS_VALUE_SECTION,
        &["oxide_pw", "oxide_db", "oxide_key"],
        REDIS_VALUE_TAIL,
    )
}

/// Builds the rename or delete command for one key.
///
/// stdin carries the password, the database index, the key, and the new name
/// for a rename.
pub fn redis_key_mutation_command(
    endpoint: &ResourceRedisEndpoint,
    os_type: &str,
    mutation: RedisKeyMutation,
) -> Option<String> {
    let (reads, tail) = match mutation {
        RedisKeyMutation::Delete => (
            vec!["oxide_pw", "oxide_db", "oxide_key"],
            REDIS_ACTION_TAIL_DELETE,
        ),
        RedisKeyMutation::Rename => (
            vec!["oxide_pw", "oxide_db", "oxide_key", "oxide_new"],
            REDIS_ACTION_TAIL_RENAME,
        ),
    };
    redis_script_for(endpoint, os_type, REDIS_ACTION_SECTION, &reads, tail)
}

/// Builds the command that creates a string key without overwriting one.
///
/// stdin carries the password, database index and key as lines, then the value
/// verbatim; `EXISTS` guards the write so a typo cannot replace live data.
pub fn redis_create_key_command(
    endpoint: &ResourceRedisEndpoint,
    os_type: &str,
    kind: RedisCreateKind,
    ttl_seconds: Option<u64>,
) -> Option<String> {
    if matches!(os_type, "Windows" | "windows") {
        return None;
    }
    let cli = redis_cli_function(endpoint)?;
    let tail = REDIS_CREATE_KEY_TEMPLATE
        .replace("{WRITE}", kind.write_script())
        .replace("{TTL}", &ttl_seconds.unwrap_or(0).to_string());
    Some(redis_script(
        REDIS_ACTION_SECTION,
        &["oxide_pw", "oxide_db", "oxide_key"],
        &cli,
        &tail,
    ))
}

/// Value shapes the set-value editor can write.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedisValueWriteKind {
    String,
    Json,
    Hash,
    List,
    Set,
    SortedSet,
}

impl RedisValueWriteKind {
    /// Maps a fetched value kind onto the write shape.
    pub fn from_value_kind(kind: ResourceRedisValueKind) -> Option<Self> {
        match kind {
            ResourceRedisValueKind::String => Some(Self::String),
            ResourceRedisValueKind::Json => Some(Self::Json),
            ResourceRedisValueKind::Hash => Some(Self::Hash),
            ResourceRedisValueKind::List => Some(Self::List),
            ResourceRedisValueKind::Set => Some(Self::Set),
            ResourceRedisValueKind::SortedSet => Some(Self::SortedSet),
            ResourceRedisValueKind::Stream | ResourceRedisValueKind::Other => None,
        }
    }

    fn replace_script(self) -> &'static str {
        match self {
            Self::String => concat!(
                "oxide_out=$(printf '%s' \"$oxide_body\" | oxide_cli_stdin -n \"$oxide_db\" -x SET \"$oxide_key\" 2>&1); ",
                "oxide_restore_ttl=0; ",
            ),
            Self::Json => concat!(
                "oxide_out=$(printf '%s' \"$oxide_body\" | oxide_cli_stdin -n \"$oxide_db\" -x JSON.SET \"$oxide_key\" '$' 2>&1); ",
                "oxide_restore_ttl=0; ",
            ),
            Self::Hash => concat!(
                "oxide_out=$(printf '%s\\n' \"$oxide_body\" | ",
                "while IFS='<TAB>' read -r oxide_field oxide_field_value; do ",
                "if [ -z \"$oxide_field\" ]; then continue; fi; ",
                "printf '%s' \"$oxide_field_value\" | oxide_cli_stdin -n \"$oxide_db\" -x HSET \"$oxide_key\" \"$oxide_field\"; echo; ",
                "done); ",
            ),
            Self::List => concat!(
                "oxide_out=$(printf '%s\\n' \"$oxide_body\" | ",
                "while IFS= read -r oxide_item; do ",
                "if [ -z \"$oxide_item\" ]; then continue; fi; ",
                "printf '%s' \"$oxide_item\" | oxide_cli_stdin -n \"$oxide_db\" -x RPUSH \"$oxide_key\"; echo; ",
                "done); ",
            ),
            Self::Set => concat!(
                "oxide_out=$(printf '%s\\n' \"$oxide_body\" | ",
                "while IFS= read -r oxide_item; do ",
                "if [ -z \"$oxide_item\" ]; then continue; fi; ",
                "printf '%s' \"$oxide_item\" | oxide_cli_stdin -n \"$oxide_db\" -x SADD \"$oxide_key\"; echo; ",
                "done); ",
            ),
            Self::SortedSet => concat!(
                "oxide_out=$(printf '%s\\n' \"$oxide_body\" | ",
                "while IFS='<TAB>' read -r oxide_member oxide_score; do ",
                "if [ -z \"$oxide_member\" ]; then continue; fi; ",
                "printf '%s' \"$oxide_member\" | oxide_cli_stdin -n \"$oxide_db\" -x ZADD \"$oxide_key\" \"$oxide_score\"; echo; ",
                "done); ",
            ),
        }
    }
}

/// Replacement writes keep the key's expiry: collection kinds rebuild the key
/// (`DEL` + inserts), so the remaining PTTL is re-applied afterwards.
const REDIS_REPLACE_VALUE_TEMPLATE: &str = concat!(
    "case \"$oxide_db\" in ''|*[!0-9]*) oxide_db=0;; esac; ",
    "oxide_body=$(cat); ",
    "if [ -z \"$oxide_key\" ]; then printf 'status\\terror\\tmissing key\\n'; else ",
    "oxide_kind=$(oxide_cli -n \"$oxide_db\" TYPE \"$oxide_key\" 2>/dev/null | head -n 1 | tr -d '\\r'); ",
    "if [ \"$oxide_kind\" = \"none\" ]; then printf 'status\\terror\\tmissing key\\n'; else ",
    "oxide_pttl=$(oxide_cli -n \"$oxide_db\" PTTL \"$oxide_key\" 2>/dev/null | head -n 1 | tr -d '\\r'); ",
    "case \"$oxide_pttl\" in ''|*[!0-9-]*) oxide_pttl=-1;; esac; ",
    "{RESET}",
    "{WRITE}",
    "case \"$oxide_out\" in ",
    "*ERR*|*WRONGTYPE*|*NOAUTH*|*WRONGPASS*|*'not an integer'*) ",
    "printf 'status\\terror\\t%s\\n' \"$(printf '%s' \"$oxide_out\" | grep -E 'ERR|WRONGTYPE|NOAUTH|WRONGPASS|not an integer' | head -n 1 | tr '\\t' ' ' | tr -d '\\r')\" ;; ",
    "*) ",
    "if [ \"{RESTORE_TTL}\" = \"1\" ] && [ \"$oxide_pttl\" -gt 0 ]; then ",
    "oxide_cli -n \"$oxide_db\" PEXPIRE \"$oxide_key\" \"$oxide_pttl\" >/dev/null 2>&1; fi; ",
    "printf 'status\\tok\\n' ;; ",
    "esac; fi; fi; "
);

/// Builds the command that replaces a value with type-appropriate writes.
///
/// stdin carries the password, database and key as lines, then the body
/// (`field<TAB>value`, `member<TAB>score`, or plain elements). String and JSON
/// values are written in place; collections are rebuilt and keep their expiry.
pub fn redis_replace_value_command(
    endpoint: &ResourceRedisEndpoint,
    os_type: &str,
    kind: RedisValueWriteKind,
) -> Option<String> {
    if matches!(os_type, "Windows" | "windows") {
        return None;
    }
    let cli = redis_cli_function(endpoint)?;
    let reset = match kind {
        RedisValueWriteKind::String | RedisValueWriteKind::Json => "",
        _ => "oxide_cli -n \"$oxide_db\" DEL \"$oxide_key\" >/dev/null 2>&1; ",
    };
    let write = kind.replace_script().replace("<TAB>", "\t");
    let tail = REDIS_REPLACE_VALUE_TEMPLATE
        .replace("{RESET}", reset)
        .replace("{WRITE}", &write)
        .replace("{RESTORE_TTL}", "1");
    Some(redis_script(
        REDIS_ACTION_SECTION,
        &["oxide_pw", "oxide_db", "oxide_key"],
        &cli,
        &tail,
    ))
}

/// Builds the stdin payload for the scripts above: one value per line.
///
/// Redis keys can hold any byte, but the scripts read one line per argument, so
/// newlines are refused instead of silently truncating the key.
pub fn redis_stdin_payload(password: &str, values: &[&str]) -> Option<Zeroizing<Vec<u8>>> {
    let mut capacity = password.len() + 1;
    for value in values {
        if value.contains(['\n', '\r']) {
            return None;
        }
        capacity += value.len() + 1;
    }
    let mut payload = Zeroizing::new(Vec::with_capacity(capacity));
    payload.extend_from_slice(password.as_bytes());
    payload.push(b'\n');
    for value in values {
        payload.extend_from_slice(value.as_bytes());
        payload.push(b'\n');
    }
    Some(payload)
}

/// Builds a payload whose last part is written verbatim, for commands that read
/// a body with `cat`. Line values stay restricted; the body may span lines.
pub fn redis_stdin_payload_with_body(
    password: &str,
    values: &[&str],
    body: &str,
) -> Option<Zeroizing<Vec<u8>>> {
    for value in values {
        if value.contains(['\n', '\r']) {
            return None;
        }
    }
    let mut capacity = password.len() + 1 + body.len();
    for value in values {
        capacity += value.len() + 1;
    }
    let mut payload = Zeroizing::new(Vec::with_capacity(capacity));
    payload.extend_from_slice(password.as_bytes());
    payload.push(b'\n');
    for value in values {
        payload.extend_from_slice(value.as_bytes());
        payload.push(b'\n');
    }
    payload.extend_from_slice(body.as_bytes());
    Some(payload)
}

/// Docker accepts `[a-zA-Z0-9][a-zA-Z0-9_.-]*` for container names; anything
/// else is refused rather than quoted into the remote command.
fn is_valid_container_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    name.len() <= 128
        && first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

pub fn parse_redis_snapshot(output: &str) -> ResourceRedisSnapshot {
    let Some(section) = extract_section(output, "REDIS") else {
        return ResourceRedisSnapshot::default();
    };

    let mut saw_client_line = false;
    let mut client_present = false;
    let mut client_missing = false;
    let mut endpoints: Vec<ResourceRedisEndpoint> = Vec::new();
    let mut published_loopback = false;

    for line in section.lines().map(str::trim_start) {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        // Markers are recognized as standalone lines only so a crafted server
        // reply cannot forge a status.
        if line == REDIS_UNAVAILABLE_MARKER {
            return ResourceRedisSnapshot {
                status: ResourceRedisStatus::Unavailable,
                endpoints,
            };
        }
        if line == REDIS_OK_LINE {
            return ResourceRedisSnapshot {
                status: ResourceRedisStatus::Available,
                endpoints,
            };
        }
        if let Some(reply) = line.strip_prefix(REDIS_ERROR_MARKER) {
            return ResourceRedisSnapshot {
                status: classify_redis_failure(clean_server_reply(reply)),
                endpoints,
            };
        }
        let Some((field, rest)) = line.split_once(['\t', ' ']) else {
            continue;
        };
        match field {
            "client" => match rest.trim() {
                "present" => {
                    saw_client_line = true;
                    client_present = true;
                }
                "missing" => {
                    saw_client_line = true;
                    client_missing = true;
                }
                _ => {}
            },
            "endpoint" => {
                let Some((kind, target)) = rest.split_once(['\t', ' ']) else {
                    continue;
                };
                let kind = match kind {
                    // Both runtimes share the `ps`/`exec` surface, so the kind only
                    // says "read through a container runtime".
                    "container" | "docker" => ResourceRedisEndpointKind::Docker,
                    "host" => ResourceRedisEndpointKind::Host,
                    _ => continue,
                };
                let target = target.trim();
                if target.is_empty() || target.chars().any(char::is_control) {
                    continue;
                }
                endpoints.push(ResourceRedisEndpoint::new(kind, target));
            }
            "published" => published_loopback = true,
            _ => {}
        }
    }

    if client_present && !published_loopback {
        endpoints.push(ResourceRedisEndpoint::new(
            ResourceRedisEndpointKind::Host,
            REDIS_PROBE_TARGET,
        ));
    }
    let status = if !endpoints.is_empty() {
        ResourceRedisStatus::Available
    } else if client_missing {
        ResourceRedisStatus::Unavailable
    } else if saw_client_line {
        ResourceRedisStatus::NoServer
    } else {
        // No discovery fields: a legacy probe already reported its own status.
        ResourceRedisStatus::Unknown
    };
    ResourceRedisSnapshot { status, endpoints }
}

pub fn parse_redis_keys(output: &str) -> ResourceRedisKeys {
    let Some(section) = extract_section(output, "REDISKEYS") else {
        return ResourceRedisKeys::default();
    };
    let mut status = ResourceRedisStatus::Unknown;
    let mut databases: Vec<ResourceRedisDatabase> = Vec::new();

    for line in section.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let field = parts.next().unwrap_or_default();
        match field {
            "status" => {
                let value = parts.next().unwrap_or_default().trim();
                let message = parts.next().map(|message| message.to_string());
                status = match value {
                    "ready" => ResourceRedisStatus::Available,
                    "auth" => ResourceRedisStatus::AuthRequired,
                    "noserver" => ResourceRedisStatus::NoServer,
                    _ => ResourceRedisStatus::Error {
                        message: message
                            .and_then(|message| clean_server_reply(&message))
                            .unwrap_or_else(|| "Redis browse failed.".to_string()),
                    },
                };
            }
            "db" => {
                let Some(index) = parts
                    .next()
                    .and_then(|index| index.trim().parse::<u32>().ok())
                else {
                    continue;
                };
                let key_count = parts
                    .next()
                    .and_then(|count| count.trim().parse::<u64>().ok())
                    .unwrap_or(0);
                databases.push(ResourceRedisDatabase {
                    index,
                    key_count,
                    keys: Vec::new(),
                });
            }
            "key" => {
                let Some(index) = parts
                    .next()
                    .and_then(|index| index.trim().parse::<u32>().ok())
                else {
                    continue;
                };
                let Some(name) = parts.next() else {
                    continue;
                };
                if name.is_empty() {
                    continue;
                }
                match databases
                    .iter_mut()
                    .find(|database| database.index == index)
                {
                    Some(database) => database.keys.push(name.to_string()),
                    None => databases.push(ResourceRedisDatabase {
                        index,
                        key_count: 0,
                        keys: vec![name.to_string()],
                    }),
                }
            }
            _ => {}
        }
    }

    databases.sort_by_key(|database| database.index);
    for database in &mut databases {
        // SCAN returns arbitrary order; sort so the tree and its row signatures
        // stay stable between refreshes.
        database.keys.sort();
        database.keys.dedup();
    }
    ResourceRedisKeys { status, databases }
}

pub fn parse_redis_value(output: &str) -> ResourceRedisValue {
    let Some(section) = extract_section(output, REDIS_VALUE_SECTION) else {
        return ResourceRedisValue::default();
    };
    let (meta, body) = match section.split_once(REDIS_BODY_MARKER) {
        Some((meta, body)) => (meta, Some(body.trim_matches(['\r', '\n']).to_string())),
        None => (section, None),
    };
    let mut value = ResourceRedisValue::default();
    for line in meta.lines() {
        let line = line.trim_end_matches('\r');
        let mut parts = line.splitn(3, '\t');
        let field = parts.next().unwrap_or_default();
        let item = parts.next().unwrap_or_default().trim();
        match field {
            "status" => {
                value.status = match item {
                    "ready" => ResourceRedisStatus::Available,
                    "missing" => ResourceRedisStatus::NoServer,
                    "auth" => ResourceRedisStatus::AuthRequired,
                    _ => ResourceRedisStatus::Error {
                        message: clean_server_reply(parts.next().unwrap_or_default())
                            .unwrap_or_else(|| "Redis value read failed.".to_string()),
                    },
                };
            }
            "type" => value.kind = ResourceRedisValueKind::from_type_reply(item),
            "rawtype" => value.raw_type = clean_server_reply(item),
            "ttl" => {
                value.ttl = match item.parse::<i64>() {
                    Ok(-1) => ResourceRedisTtl::Never,
                    Ok(-2) => ResourceRedisTtl::Missing,
                    Ok(seconds) if seconds >= 0 => ResourceRedisTtl::Seconds(seconds as u64),
                    _ => ResourceRedisTtl::Unknown,
                };
            }
            "usage" => value.memory_usage = item.parse().ok(),
            "length" => value.length = item.parse().ok(),
            _ => {}
        }
    }
    value.body = body.unwrap_or_default();
    value
}

pub fn parse_redis_key_mutation(output: &str) -> ResourceRedisStatus {
    let Some(section) = extract_section(output, REDIS_ACTION_SECTION) else {
        return ResourceRedisStatus::Unknown;
    };
    for line in section.lines() {
        let line = line.trim_end_matches('\r');
        let mut parts = line.splitn(3, '\t');
        if parts.next() != Some("status") {
            continue;
        }
        return match parts.next().unwrap_or_default().trim() {
            "ok" => ResourceRedisStatus::Available,
            _ => ResourceRedisStatus::Error {
                message: clean_server_reply(parts.next().unwrap_or_default())
                    .unwrap_or_else(|| "Redis key action failed.".to_string()),
            },
        };
    }
    ResourceRedisStatus::Unknown
}

/// Structured rows for the value viewer and the set-value editor.
///
/// Hash and sorted-set rows carry their second column (value/score); list and
/// set rows are single-column. String/JSON documents have no rows.
pub fn redis_value_rows(value: &ResourceRedisValue) -> Vec<(String, Option<String>)> {
    match value.kind {
        ResourceRedisValueKind::Hash | ResourceRedisValueKind::SortedSet => {
            redis_value_pairs(value)
        }
        ResourceRedisValueKind::List | ResourceRedisValueKind::Set => value
            .body
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| (line.to_string(), None))
            .collect(),
        ResourceRedisValueKind::String
        | ResourceRedisValueKind::Json
        | ResourceRedisValueKind::Stream
        | ResourceRedisValueKind::Other => Vec::new(),
    }
}

/// Pairs the alternating lines `redis-cli` prints for two-element replies.
///
/// A non-interactive `redis-cli` prints array replies one element per line, so
/// `HGETALL` and `ZRANGE ... WITHSCORES` arrive as field/value or member/score
/// pairs without separators. A trailing odd line is kept as a keyless value.
fn redis_value_pairs(value: &ResourceRedisValue) -> Vec<(String, Option<String>)> {
    let lines = value
        .body
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    match value.kind {
        ResourceRedisValueKind::Hash | ResourceRedisValueKind::SortedSet => lines
            .chunks(2)
            .map(|chunk| {
                (
                    chunk[0].to_string(),
                    chunk.get(1).map(|value| value.to_string()),
                )
            })
            .collect(),
        _ => lines
            .into_iter()
            .map(|line| (line.to_string(), None))
            .collect(),
    }
}

/// Renders the text view: collection pairs are joined by a tab so a field and
/// its value stay on one row, and other types keep redis-cli's raw reply.
pub fn redis_value_text(value: &ResourceRedisValue) -> String {
    match value.kind {
        // A JSON document is already the most readable form.
        ResourceRedisValueKind::Json => value.body.clone(),
        ResourceRedisValueKind::Hash | ResourceRedisValueKind::SortedSet => {
            redis_value_pairs(value)
                .into_iter()
                .map(|(member, second)| match second {
                    Some(second) => format!("{member}\t{second}"),
                    None => member,
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => value.body.clone(),
    }
}

/// Renders a value as JSON for the structured view.
///
/// Returns `None` for types without a faithful JSON shape (streams and unknown
/// types); the panel falls back to the raw text view.
pub fn redis_value_json(value: &ResourceRedisValue) -> Option<String> {
    let rendered = match value.kind {
        ResourceRedisValueKind::String => {
            match serde_json::from_str::<serde_json::Value>(&value.body) {
                Ok(parsed) => serde_json::to_string_pretty(&parsed).ok()?,
                Err(_) => serde_json::to_string(&value.body).ok()?,
            }
        }
        ResourceRedisValueKind::Hash => {
            let mut fields = redis_value_pairs(value);
            // Hash fields arrive in server order; sorting keeps the JSON view
            // stable between refreshes.
            fields.sort_by(|left, right| left.0.cmp(&right.0));
            let mut object = serde_json::Map::new();
            for (field, field_value) in fields {
                object.insert(
                    field,
                    serde_json::Value::String(field_value.unwrap_or_default()),
                );
            }
            serde_json::to_string_pretty(&serde_json::Value::Object(object)).ok()?
        }
        ResourceRedisValueKind::List | ResourceRedisValueKind::Set => {
            let mut items = value
                .body
                .lines()
                .filter(|line| !line.is_empty())
                .map(|line| line.to_string())
                .collect::<Vec<_>>();
            // Lists keep their server order; sets have none, so sort them.
            if value.kind == ResourceRedisValueKind::Set {
                items.sort();
            }
            let items = items
                .into_iter()
                .map(serde_json::Value::String)
                .collect::<Vec<_>>();
            serde_json::to_string_pretty(&serde_json::Value::Array(items)).ok()?
        }
        ResourceRedisValueKind::SortedSet => {
            let members = redis_value_pairs(value)
                .into_iter()
                .map(|(member, score)| {
                    let score = score
                        .as_deref()
                        .and_then(|score| score.parse::<f64>().ok())
                        .filter(|score| score.is_finite())
                        .map(|score| serde_json::json!(score))
                        .unwrap_or(serde_json::Value::Null);
                    serde_json::json!({ "member": member, "score": score })
                })
                .collect::<Vec<_>>();
            serde_json::to_string_pretty(&serde_json::Value::Array(members)).ok()?
        }
        ResourceRedisValueKind::Json => {
            match serde_json::from_str::<serde_json::Value>(&value.body) {
                Ok(parsed) => serde_json::to_string_pretty(&parsed).ok()?,
                Err(_) => serde_json::to_string(&value.body).ok()?,
            }
        }
        ResourceRedisValueKind::Stream | ResourceRedisValueKind::Other => return None,
    };
    Some(rendered)
}

/// Builds the colon-separated key tree for one database.
pub fn redis_key_tree(keys: &[String]) -> Vec<ResourceRedisKeyNode> {
    let mut root: TrieNode = TrieNode::default();
    for key in keys {
        if key.is_empty() {
            continue;
        }
        root.insert(key);
    }
    root.into_nodes("")
}

#[derive(Default)]
struct TrieNode {
    is_key: bool,
    children: BTreeMap<String, TrieNode>,
}

impl TrieNode {
    fn insert(&mut self, key: &str) {
        let mut node = self;
        for segment in key.split(':') {
            node = node.children.entry(segment.to_string()).or_default();
        }
        node.is_key = true;
    }

    fn into_nodes(self, prefix: &str) -> Vec<ResourceRedisKeyNode> {
        self.children
            .into_iter()
            .map(|(label, child)| {
                let path = if prefix.is_empty() {
                    label.clone()
                } else {
                    format!("{prefix}:{label}")
                };
                let is_key = child.is_key;
                let children = child.into_nodes(&path);
                let key_count =
                    usize::from(is_key) + children.iter().map(|node| node.key_count).sum::<usize>();
                ResourceRedisKeyNode {
                    label,
                    path,
                    is_key,
                    key_count,
                    children,
                }
            })
            .collect()
    }
}

/// Redis replies are server-controlled strings, unlike docker's local binary
/// output: keep the first line only, replace control characters with spaces,
/// and bound the length before the text reaches UI or diagnostics.
fn clean_server_reply(value: &str) -> Option<String> {
    let line = value.trim_start_matches(['\t', ' ']).lines().next()?;
    let cleaned: String = line
        .trim()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(180)
        .collect();
    let cleaned = cleaned.trim();
    (!cleaned.is_empty()).then(|| cleaned.to_string())
}

fn classify_redis_failure(reply: Option<String>) -> ResourceRedisStatus {
    let Some(reply) = reply else {
        return ResourceRedisStatus::Error {
            message: "Redis probe failed.".to_string(),
        };
    };
    // Match the reply-code prefix, not the sentence: Valkey/KeyDB and redis
    // releases reword the text but keep the error code.
    let lowered = reply.to_lowercase();
    if lowered.starts_with("noauth") {
        ResourceRedisStatus::AuthRequired
    } else if lowered.contains("connection refused") {
        ResourceRedisStatus::NoServer
    } else {
        ResourceRedisStatus::Error { message: reply }
    }
}

fn extract_section<'a>(output: &'a str, name: &str) -> Option<&'a str> {
    let start = format!("==={name}===");
    let end = format!("==={name}_END===");
    let after_start = output.split_once(&start)?.1;
    Some(after_start.split_once(&end)?.0.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys_output(body: &str) -> String {
        format!("===REDISKEYS===\n{body}===REDISKEYS_END===")
    }

    #[test]
    fn legacy_probe_markers_still_map_to_states() {
        for (case, output, expected) in [
            (
                "pong",
                "===REDIS===\nREDIS_OK\n===REDIS_END===",
                ResourceRedisStatus::Available,
            ),
            (
                "missing_client_binary",
                "===REDIS===\n__OXIDE_REDIS_UNAVAILABLE__\n===REDIS_END===",
                ResourceRedisStatus::Unavailable,
            ),
            (
                "auth_required",
                concat!(
                    "===REDIS===\n",
                    "__OXIDE_REDIS_ERROR__\tNOAUTH Authentication required.\n",
                    "===REDIS_END===",
                ),
                ResourceRedisStatus::AuthRequired,
            ),
            (
                "valkey_reworded_auth_reply_keeps_error_code",
                concat!(
                    "===REDIS===\n",
                    "__OXIDE_REDIS_ERROR__\tNOAUTH HELLO must be called with AUTH\n",
                    "===REDIS_END===",
                ),
                ResourceRedisStatus::AuthRequired,
            ),
            (
                "nothing_listening",
                concat!(
                    "===REDIS===\n",
                    "__OXIDE_REDIS_ERROR__\tCould not connect to Redis at 127.0.0.1:6379: Connection refused\n",
                    "===REDIS_END===",
                ),
                ResourceRedisStatus::NoServer,
            ),
        ] {
            let snapshot = parse_redis_snapshot(output);
            assert_eq!(snapshot.status, expected, "{case}");
            assert!(snapshot.endpoints.is_empty(), "{case}");
        }
    }

    #[test]
    fn discovery_lists_containers_and_suppresses_the_owned_loopback() {
        let with_published_port = concat!(
            "===REDIS===\n",
            "client\tmissing\n",
            "endpoint\tdocker\tredis-cache\n",
            "published\tredis-cache\n",
            "===REDIS_END===",
        );
        let snapshot = parse_redis_snapshot(with_published_port);
        assert_eq!(snapshot.status, ResourceRedisStatus::Available);
        assert_eq!(
            snapshot.endpoints,
            vec![ResourceRedisEndpoint::new(
                ResourceRedisEndpointKind::Docker,
                "redis-cache"
            )]
        );

        let without_published_port = concat!(
            "===REDIS===\n",
            "client\tpresent\n",
            "endpoint\tdocker\tredis-cache\n",
            "===REDIS_END===",
        );
        assert_eq!(
            parse_redis_snapshot(without_published_port).endpoints,
            vec![
                ResourceRedisEndpoint::new(ResourceRedisEndpointKind::Docker, "redis-cache"),
                ResourceRedisEndpoint::new(ResourceRedisEndpointKind::Host, REDIS_PROBE_TARGET),
            ]
        );
    }

    #[test]
    fn discovery_without_client_or_containers_reports_unavailable() {
        let snapshot = parse_redis_snapshot(concat!(
            "===REDIS===\n",
            "client\tmissing\n",
            "===REDIS_END===",
        ));
        assert_eq!(snapshot.status, ResourceRedisStatus::Unavailable);
        assert!(snapshot.endpoints.is_empty());

        // A host client with no container still offers the loopback endpoint;
        // whether anything listens there is decided by the browse step.
        let host_only = parse_redis_snapshot(concat!(
            "===REDIS===\n",
            "client\tpresent\n",
            "===REDIS_END===",
        ));
        assert_eq!(host_only.status, ResourceRedisStatus::Available);
        assert_eq!(
            host_only.endpoints,
            vec![ResourceRedisEndpoint::new(
                ResourceRedisEndpointKind::Host,
                REDIS_PROBE_TARGET
            )]
        );
    }

    #[test]
    fn parser_sanitizes_server_controlled_text() {
        let output = concat!(
            "===REDIS===\n",
            "__OXIDE_REDIS_ERROR__\tERR injected\u{7}bell\u{1b}[31mcolor\nsecond line\n",
            "===REDIS_END===",
        );
        assert_eq!(
            parse_redis_snapshot(output).status,
            ResourceRedisStatus::Error {
                message: "ERR injected bell [31mcolor".to_string(),
            }
        );
    }

    #[test]
    fn parser_rejects_forged_markers_inside_server_replies() {
        // A hostile server reply must not flip the probe into a status marker;
        // only standalone marker lines are recognized.
        let output = concat!(
            "===REDIS===\n",
            "__OXIDE_REDIS_ERROR__\tERR crafted __OXIDE_REDIS_UNAVAILABLE__ payload\n",
            "===REDIS_END===",
        );
        assert_eq!(
            parse_redis_snapshot(output).status,
            ResourceRedisStatus::Error {
                message: "ERR crafted __OXIDE_REDIS_UNAVAILABLE__ payload".to_string(),
            }
        );
    }

    #[test]
    fn empty_capture_stays_unknown() {
        assert_eq!(
            parse_redis_snapshot("===REDIS===\n===REDIS_END===").status,
            ResourceRedisStatus::Unknown
        );
        assert_eq!(
            parse_redis_snapshot("no section at all").status,
            ResourceRedisStatus::Unknown
        );
        assert_eq!(
            parse_redis_keys("no section at all").status,
            ResourceRedisStatus::Unknown
        );
    }

    #[test]
    fn keys_parser_reads_databases_and_keeps_tabbed_key_names() {
        let output = keys_output(concat!(
            "status\tready\n",
            "db\t2\t3\n",
            "key\t2\tsession:9\n",
            "key\t2\tsession:1\n",
            "key\t2\thas\ttab\n",
            "db\t0\t1\n",
            "key\t0\tuser:1\n",
        ));
        let keys = parse_redis_keys(&output);
        assert_eq!(keys.status, ResourceRedisStatus::Available);
        assert_eq!(
            keys.databases,
            vec![
                ResourceRedisDatabase {
                    index: 0,
                    key_count: 1,
                    keys: vec!["user:1".to_string()],
                },
                ResourceRedisDatabase {
                    index: 2,
                    key_count: 3,
                    keys: vec![
                        "has\ttab".to_string(),
                        "session:1".to_string(),
                        "session:9".to_string(),
                    ],
                },
            ]
        );
    }

    #[test]
    fn keys_parser_maps_auth_and_failure_states() {
        for (case, body, expected) in [
            ("auth", "status\tauth\n", ResourceRedisStatus::AuthRequired),
            (
                "noserver",
                "status\tnoserver\n",
                ResourceRedisStatus::NoServer,
            ),
            (
                "error_with_message",
                "status\terror\tWRONGPASS invalid username-password pair\n",
                ResourceRedisStatus::Error {
                    message: "WRONGPASS invalid username-password pair".to_string(),
                },
            ),
            (
                "error_without_message",
                "status\terror\n",
                ResourceRedisStatus::Error {
                    message: "Redis browse failed.".to_string(),
                },
            ),
        ] {
            let keys = parse_redis_keys(&keys_output(body));
            assert_eq!(keys.status, expected, "{case}");
            assert!(keys.databases.is_empty(), "{case}");
        }
    }

    #[test]
    fn keys_commands_keep_the_password_out_of_the_command_line() {
        let host = redis_keys_command(
            &ResourceRedisEndpoint::new(ResourceRedisEndpointKind::Host, REDIS_PROBE_TARGET),
            "Linux",
        )
        .expect("host endpoints browse on POSIX hosts");
        assert!(host.contains("IFS= read -r oxide_pw"));
        assert!(host.contains("REDISCLI_AUTH=$oxide_pw redis-cli"));
        assert!(host.contains("PING"));
        assert!(host.contains("INFO keyspace"));
        assert!(!host.contains("-a "));
        assert!(!host.contains("--askpass"));

        let docker = redis_keys_command(
            &ResourceRedisEndpoint::new(ResourceRedisEndpointKind::Docker, "redis-cache"),
            "Linux",
        )
        .expect("docker endpoints browse through docker exec");
        assert!(docker.contains("$oxide_runtime exec -e REDISCLI_AUTH redis-cache redis-cli"));
        assert!(docker.contains("else $oxide_runtime exec redis-cache redis-cli"));
        assert!(docker.contains("oxide_runtime=podman"));
        assert!(!docker.contains("-a "));
    }

    #[test]
    fn keys_commands_refuse_unvalidated_container_names() {
        for name in ["", "redis; rm -rf /", "$(id)", "redis\ncache", "-rf"] {
            assert!(
                redis_keys_command(
                    &ResourceRedisEndpoint::new(ResourceRedisEndpointKind::Docker, name),
                    "Linux",
                )
                .is_none(),
                "{name:?} must not be interpolated into the remote command"
            );
        }
    }

    #[test]
    fn keys_commands_require_a_posix_shell() {
        assert!(
            redis_keys_command(
                &ResourceRedisEndpoint::new(ResourceRedisEndpointKind::Host, REDIS_PROBE_TARGET),
                "Windows",
            )
            .is_none()
        );
    }

    #[test]
    fn keys_command_enumerates_every_configured_database() {
        let command = redis_keys_command(
            &ResourceRedisEndpoint::new(ResourceRedisEndpointKind::Host, REDIS_PROBE_TARGET),
            "Linux",
        )
        .expect("host endpoints browse on POSIX hosts");
        // Empty databases are synthesized so the rail can show them.
        assert!(command.contains("CONFIG GET databases"));
        assert!(command.contains("while [ \"$oxide_i\" -lt \"$oxide_dbs\" ]"));
        assert!(command.contains("oxide_cli -n \"$oxide_i\" --scan"));
    }

    #[test]
    fn value_parser_reads_metadata_and_body() {
        let output = concat!(
            "===REDISVALUE===\n",
            "status\tready\n",
            "type\thash\n",
            "ttl\t87\n",
            "usage\t142\n",
            "length\t2\n",
            "===REDIS_BODY===\n",
            "name\tjaime\n",
            "scope\tadmin\n",
            "===REDISVALUE_END===",
        );
        let value = parse_redis_value(output);
        assert_eq!(value.status, ResourceRedisStatus::Available);
        assert_eq!(value.kind, ResourceRedisValueKind::Hash);
        assert_eq!(value.ttl, ResourceRedisTtl::Seconds(87));
        assert_eq!(value.memory_usage, Some(142));
        assert_eq!(value.length, Some(2));
        assert_eq!(value.body, "name\tjaime\nscope\tadmin");
    }

    #[test]
    fn value_parser_maps_ttl_states() {
        for (line, expected) in [
            ("ttl\t-1\r\n", ResourceRedisTtl::Never),
            ("ttl\t-2\r\n", ResourceRedisTtl::Missing),
            ("ttl\t0\r\n", ResourceRedisTtl::Seconds(0)),
            ("ttl\tunknown\r\n", ResourceRedisTtl::Unknown),
        ] {
            let output = format!("===REDISVALUE===\nstatus\tready\n{line}===REDISVALUE_END===");
            assert_eq!(parse_redis_value(&output).ttl, expected, "{line:?}");
        }
    }

    #[test]
    fn value_parser_maps_missing_and_auth_states() {
        let missing = parse_redis_value("===REDISVALUE===\nstatus\tmissing\n===REDISVALUE_END===");
        assert_eq!(missing.status, ResourceRedisStatus::NoServer);
        assert!(missing.body.is_empty());

        let auth = parse_redis_value("===REDISVALUE===\nstatus\tauth\n===REDISVALUE_END===");
        assert_eq!(auth.status, ResourceRedisStatus::AuthRequired);

        let error = parse_redis_value(
            "===REDISVALUE===\nstatus\terror\tERR unknown command 'TYPE'\n===REDISVALUE_END===",
        );
        assert_eq!(
            error.status,
            ResourceRedisStatus::Error {
                message: "ERR unknown command 'TYPE'".to_string(),
            }
        );
    }

    #[test]
    fn value_body_cannot_forge_metadata() {
        // A stored string containing marker-shaped lines stays inside the body.
        let output = concat!(
            "===REDISVALUE===\n",
            "status\tready\n",
            "type\tstring\n",
            "length\t9\n",
            "===REDIS_BODY===\n",
            "status\tok\n",
            "ttl\t-1\n",
            "===REDISVALUE_END===",
        );
        let value = parse_redis_value(output);
        assert_eq!(value.ttl, ResourceRedisTtl::Unknown);
        assert_eq!(value.body, "status\tok\nttl\t-1");
    }

    #[test]
    fn value_json_shapes_match_each_collection_type() {
        let hash = ResourceRedisValue {
            kind: ResourceRedisValueKind::Hash,
            // redis-cli prints HGETALL as alternating field/value lines.
            body: "b\n2\na\n1".to_string(),
            ..Default::default()
        };
        assert_eq!(
            redis_value_json(&hash).unwrap(),
            "{\n  \"a\": \"1\",\n  \"b\": \"2\"\n}"
        );

        let list = ResourceRedisValue {
            kind: ResourceRedisValueKind::List,
            body: "first\nsecond".to_string(),
            ..Default::default()
        };
        assert_eq!(
            redis_value_json(&list).unwrap(),
            "[\n  \"first\",\n  \"second\"\n]"
        );

        let zset = ResourceRedisValue {
            kind: ResourceRedisValueKind::SortedSet,
            body: "alpha\n1.5\nbeta\ninf".to_string(),
            ..Default::default()
        };
        assert_eq!(
            redis_value_json(&zset).unwrap(),
            "[\n  {\n    \"member\": \"alpha\",\n    \"score\": 1.5\n  },\n  {\n    \"member\": \"beta\",\n    \"score\": null\n  }\n]"
        );

        let json_string = ResourceRedisValue {
            kind: ResourceRedisValueKind::String,
            body: "{\"a\":1}".to_string(),
            ..Default::default()
        };
        assert_eq!(redis_value_json(&json_string).unwrap(), "{\n  \"a\": 1\n}");
        let plain_string = ResourceRedisValue {
            kind: ResourceRedisValueKind::String,
            body: "hello".to_string(),
            ..Default::default()
        };
        assert_eq!(redis_value_json(&plain_string).unwrap(), "\"hello\"");
        let stream = ResourceRedisValue {
            kind: ResourceRedisValueKind::Stream,
            ..Default::default()
        };
        assert!(redis_value_json(&stream).is_none());
    }

    #[test]
    fn text_view_pairs_alternating_collection_lines() {
        let hash = ResourceRedisValue {
            kind: ResourceRedisValueKind::Hash,
            body: "name\njaime\nscope\nadmin".to_string(),
            ..Default::default()
        };
        assert_eq!(redis_value_text(&hash), "name\tjaime\nscope\tadmin");

        let zset = ResourceRedisValue {
            kind: ResourceRedisValueKind::SortedSet,
            body: "alpha\n1.5\nbeta\n2".to_string(),
            ..Default::default()
        };
        assert_eq!(redis_value_text(&zset), "alpha\t1.5\nbeta\t2");

        // Lists and strings keep redis-cli's raw reply.
        let list = ResourceRedisValue {
            kind: ResourceRedisValueKind::List,
            body: "first\nsecond".to_string(),
            ..Default::default()
        };
        assert_eq!(redis_value_text(&list), "first\nsecond");

        // An odd trailing line is kept rather than dropped.
        let truncated_hash = ResourceRedisValue {
            kind: ResourceRedisValueKind::Hash,
            body: "field\nvalue\ndangling".to_string(),
            ..Default::default()
        };
        assert_eq!(redis_value_text(&truncated_hash), "field\tvalue\ndangling");
    }

    #[test]
    fn create_key_command_guards_existing_keys_and_scopes_the_database() {
        let endpoint =
            ResourceRedisEndpoint::new(ResourceRedisEndpointKind::Host, REDIS_PROBE_TARGET);
        for (kind, write) in [
            (
                RedisCreateKind::String,
                "oxide_cli_stdin -n \"$oxide_db\" -x SET \"$oxide_key\"",
            ),
            (
                RedisCreateKind::Json,
                "oxide_cli_stdin -n \"$oxide_db\" -x SET \"$oxide_key\"",
            ),
            (
                RedisCreateKind::Hash,
                "oxide_cli_stdin -n \"$oxide_db\" -x HSET \"$oxide_key\" \"$oxide_field\"",
            ),
            (
                RedisCreateKind::List,
                "oxide_cli_stdin -n \"$oxide_db\" -x RPUSH \"$oxide_key\"",
            ),
            (
                RedisCreateKind::Set,
                "oxide_cli_stdin -n \"$oxide_db\" -x SADD \"$oxide_key\"",
            ),
            (
                RedisCreateKind::SortedSet,
                "oxide_cli_stdin -n \"$oxide_db\" -x ZADD \"$oxide_key\" \"$oxide_score\"",
            ),
        ] {
            let command = redis_create_key_command(&endpoint, "Linux", kind, Some(120)).unwrap();
            assert!(
                command.contains("oxide_cli -n \"$oxide_db\" EXISTS \"$oxide_key\""),
                "{kind:?} must guard existing keys"
            );
            assert!(command.contains(write), "{kind:?} write command");
            assert!(
                command.contains("EXPIRE \"$oxide_key\" \"120\""),
                "{kind:?} applies the ttl"
            );
            assert!(!command.contains("-a "), "{kind:?} never uses -a");
            assert!(redis_create_key_command(&endpoint, "Windows", kind, Some(1)).is_none());
        }
        // Without a ttl the script skips EXPIRE entirely.
        let no_ttl =
            redis_create_key_command(&endpoint, "Linux", RedisCreateKind::String, None).unwrap();
        assert!(!no_ttl.contains("{TTL}"));
        assert!(no_ttl.contains("[ \"0\" -gt 0 ]"));
    }

    #[test]
    fn body_payload_keeps_lines_and_allows_multiline_values() {
        let payload = redis_stdin_payload_with_body("pw", &["9", "key"], "{\n  \"a\": 1\n}")
            .expect("body payloads accept newlines");
        assert_eq!(
            payload.as_slice(),
            b"pw\n9\nkey\n{\n  \"a\": 1\n}".as_slice()
        );
        // Line values stay single-line even when a body is present.
        assert!(redis_stdin_payload_with_body("pw", &["a\nb"], "body").is_none());
    }

    #[test]
    fn value_parser_reads_redisjson_and_keeps_unknown_module_types_readable() {
        let json = parse_redis_value(concat!(
            "===REDISVALUE===\n",
            "status\tready\ntype\tjson\nttl\t-1\nusage\t120\n",
            "===REDIS_BODY===\n",
            "{\"a\":1}\n",
            "===REDISVALUE_END===",
        ));
        assert_eq!(json.kind, ResourceRedisValueKind::Json);
        assert_eq!(json.body, "{\"a\":1}");
        assert_eq!(redis_value_json(&json).unwrap(), "{\n  \"a\": 1\n}");

        // A module type we cannot read still reports its name instead of an
        // error banner in the dialog.
        let timeseries = parse_redis_value(concat!(
            "===REDISVALUE===\n",
            "status\tready\ntype\tother\nrawtype\tTimeseries\n",
            "===REDIS_BODY===\n",
            "===REDISVALUE_END===",
        ));
        assert_eq!(timeseries.status, ResourceRedisStatus::Available);
        assert_eq!(timeseries.kind, ResourceRedisValueKind::Other);
        assert_eq!(timeseries.raw_type.as_deref(), Some("Timeseries"));
        assert!(redis_value_rows(&timeseries).is_empty());
    }

    #[test]
    fn value_rows_feed_the_viewer_and_the_editor() {
        let hash = ResourceRedisValue {
            kind: ResourceRedisValueKind::Hash,
            body: "name\njaime\nscope\nadmin".to_string(),
            ..Default::default()
        };
        assert_eq!(
            redis_value_rows(&hash),
            vec![
                ("name".to_string(), Some("jaime".to_string())),
                ("scope".to_string(), Some("admin".to_string())),
            ]
        );
        let zset = ResourceRedisValue {
            kind: ResourceRedisValueKind::SortedSet,
            body: "alpha\n1.5\nbeta\n2".to_string(),
            ..Default::default()
        };
        assert_eq!(
            redis_value_rows(&zset),
            vec![
                ("alpha".to_string(), Some("1.5".to_string())),
                ("beta".to_string(), Some("2".to_string())),
            ]
        );
        let list = ResourceRedisValue {
            kind: ResourceRedisValueKind::List,
            body: "one\ntwo".to_string(),
            ..Default::default()
        };
        assert_eq!(
            redis_value_rows(&list),
            vec![("one".to_string(), None), ("two".to_string(), None),]
        );
    }

    #[test]
    fn replace_value_command_writes_each_shape_and_keeps_the_ttl() {
        let endpoint =
            ResourceRedisEndpoint::new(ResourceRedisEndpointKind::Host, REDIS_PROBE_TARGET);
        let hash =
            redis_replace_value_command(&endpoint, "Linux", RedisValueWriteKind::Hash).unwrap();
        assert!(hash.contains("DEL \"$oxide_key\""));
        assert!(hash.contains("oxide_cli_stdin -n \"$oxide_db\" -x HSET"));
        assert!(hash.contains("PEXPIRE \"$oxide_key\" \"$oxide_pttl\""));

        let json =
            redis_replace_value_command(&endpoint, "Linux", RedisValueWriteKind::Json).unwrap();
        assert!(!json.contains("DEL \"$oxide_key\""));
        assert!(json.contains("oxide_cli_stdin -n \"$oxide_db\" -x JSON.SET \"$oxide_key\" '$'"));

        for (kind, needle) in [
            (RedisValueWriteKind::String, "-x SET"),
            (RedisValueWriteKind::List, "-x RPUSH"),
            (RedisValueWriteKind::Set, "-x SADD"),
            (RedisValueWriteKind::SortedSet, "-x ZADD"),
        ] {
            let command = redis_replace_value_command(&endpoint, "Linux", kind).unwrap();
            assert!(command.contains(needle), "{kind:?}");
        }
        assert!(
            redis_replace_value_command(&endpoint, "Windows", RedisValueWriteKind::String)
                .is_none()
        );
    }

    #[test]
    fn mutation_parser_reports_success_and_server_errors() {
        assert_eq!(
            parse_redis_key_mutation("===REDISACTION===\nstatus\tok\n===REDISACTION_END==="),
            ResourceRedisStatus::Available
        );
        assert_eq!(
            parse_redis_key_mutation(
                "===REDISACTION===\nstatus\terror\tERR no such key\n===REDISACTION_END==="
            ),
            ResourceRedisStatus::Error {
                message: "ERR no such key".to_string(),
            }
        );
        assert_eq!(
            parse_redis_key_mutation("no section"),
            ResourceRedisStatus::Unknown
        );
    }

    #[test]
    fn value_and_mutation_commands_carry_no_key_or_password_on_the_command_line() {
        let endpoint =
            ResourceRedisEndpoint::new(ResourceRedisEndpointKind::Host, REDIS_PROBE_TARGET);
        for command in [
            redis_value_command(&endpoint, "Linux").unwrap(),
            redis_key_mutation_command(&endpoint, "Linux", RedisKeyMutation::Delete).unwrap(),
            redis_key_mutation_command(&endpoint, "Linux", RedisKeyMutation::Rename).unwrap(),
        ] {
            assert!(command.contains("IFS= read -r oxide_pw"));
            assert!(command.contains("oxide_cli"));
            assert!(!command.contains("-a "));
            assert!(!command.contains("--askpass"));
        }
        let value_command = redis_value_command(&endpoint, "Linux").unwrap();
        assert!(value_command.contains("IFS= read -r oxide_db"));
        assert!(value_command.contains("IFS= read -r oxide_key"));
        // A key lives in one database: every read is scoped to the index the
        // caller supplied, otherwise db0 keys only would resolve.
        assert!(value_command.contains("oxide_cli -n \"$oxide_db\" TYPE \"$oxide_key\""));
        assert!(value_command.contains("oxide_cli -n \"$oxide_db\" MEMORY USAGE"));
        assert!(value_command.contains("oxide_cli -n \"$oxide_db\" HGETALL"));
        for call in [
            "TYPE",
            "TTL",
            "MEMORY USAGE",
            "STRLEN",
            "GET",
            "HLEN",
            "HGETALL",
            "LLEN",
            "LRANGE",
            "SCARD",
            "SMEMBERS",
            "ZCARD",
            "ZRANGE",
            "XLEN",
        ] {
            assert!(
                value_command.contains(&format!("oxide_cli -n \"$oxide_db\" {call} ")),
                "{call} must read the key's database"
            );
        }
        let rename =
            redis_key_mutation_command(&endpoint, "Linux", RedisKeyMutation::Rename).unwrap();
        assert!(rename.contains("IFS= read -r oxide_db"));
        assert!(rename.contains("IFS= read -r oxide_new"));
        assert!(rename.contains("oxide_cli -n \"$oxide_db\" RENAME \"$oxide_key\" \"$oxide_new\""));
        assert!(redis_value_command(&endpoint, "Windows").is_none());
    }

    #[test]
    fn stdin_payload_is_line_delimited_and_rejects_multiline_values() {
        let payload = redis_stdin_payload("s3cr3t", &["user:1"]).unwrap();
        assert_eq!(payload.as_slice(), b"s3cr3t\nuser:1\n");
        let rename = redis_stdin_payload("pw", &["9", "old", "new"]).unwrap();
        assert_eq!(rename.as_slice(), b"pw\n9\nold\nnew\n");
        assert!(redis_stdin_payload("pw", &["a\nb"]).is_none());
    }

    #[test]
    fn key_tree_groups_colon_separated_prefixes() {
        let keys = vec![
            "user:1".to_string(),
            "user:2".to_string(),
            "user:profile:1".to_string(),
            "user".to_string(),
            "session".to_string(),
        ];
        let tree = redis_key_tree(&keys);
        assert_eq!(
            tree.iter()
                .map(|node| node.label.as_str())
                .collect::<Vec<_>>(),
            vec!["session", "user"]
        );
        let user = &tree[1];
        assert!(user.is_key);
        assert_eq!(user.key_count, 4);
        assert_eq!(user.path, "user");
        assert_eq!(
            user.children
                .iter()
                .map(|node| (node.label.as_str(), node.key_count, node.is_key))
                .collect::<Vec<_>>(),
            vec![("1", 1, true), ("2", 1, true), ("profile", 1, false)]
        );
        assert_eq!(user.children[2].path, "user:profile");
        assert_eq!(user.children[2].children[0].path, "user:profile:1");
    }

    #[test]
    fn key_tree_keeps_empty_segments_inside_a_key() {
        let tree = redis_key_tree(&["a::b".to_string(), "::".to_string()]);
        assert_eq!(
            tree.iter()
                .map(|node| node.label.as_str())
                .collect::<Vec<_>>(),
            vec!["", "a"]
        );
        let bare = &tree[0];
        assert!(!bare.is_key);
        assert_eq!(bare.key_count, 1);
        assert_eq!(bare.children[0].label, "");
        assert!(bare.children[0].children[0].is_key);
        let prefixed = &tree[1];
        assert_eq!(prefixed.children[0].label, "");
        assert_eq!(prefixed.children[0].children[0].label, "b");
        assert!(prefixed.children[0].children[0].is_key);
        assert_eq!(prefixed.key_count, 1);
    }
}
