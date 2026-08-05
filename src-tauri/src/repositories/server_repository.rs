use sqlx::{sqlite::SqliteRow, Row, SqlitePool};

use crate::{
    domain::server::{AuthKind, ServerProfile, StoredHostKey},
    error::{AppError, AppResult},
};

const INSERT_SERVER: &str =
    "INSERT INTO servers (id,name,host,port,username,auth_kind,credential_id) VALUES (?,?,?,?,?,?,?)";
const LIST_SERVERS: &str =
    "SELECT id,name,host,port,username,auth_kind,credential_id FROM servers ORDER BY name COLLATE NOCASE,id";
const GET_SERVER: &str =
    "SELECT id,name,host,port,username,auth_kind,credential_id FROM servers WHERE id = ?";
const UPDATE_SERVER_HOST: &str = "UPDATE servers SET host=? WHERE id=?";
const UPSERT_HOST_KEY: &str =
    "INSERT INTO host_keys (server_id,algorithm,fingerprint_sha256,raw_key_base64) VALUES (?,?,?,?) \
     ON CONFLICT(server_id) DO UPDATE SET algorithm=excluded.algorithm,fingerprint_sha256=excluded.fingerprint_sha256,raw_key_base64=excluded.raw_key_base64,trusted_at=CURRENT_TIMESTAMP";
const GET_HOST_KEY: &str =
    "SELECT server_id,algorithm,fingerprint_sha256,raw_key_base64 FROM host_keys WHERE server_id = ?";

#[derive(Clone)]
pub struct ServerRepository {
    pool: SqlitePool,
}

impl ServerRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn insert(&self, server: &ServerProfile) -> AppResult<()> {
        sqlx::query(INSERT_SERVER)
            .bind(&server.id)
            .bind(&server.name)
            .bind(&server.host)
            .bind(i64::from(server.port))
            .bind(&server.username)
            .bind(server.auth_kind.as_str())
            .bind(&server.credential_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list(&self) -> AppResult<Vec<ServerProfile>> {
        sqlx::query(LIST_SERVERS)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(map_server)
            .collect()
    }

    pub async fn get(&self, id: &str) -> AppResult<Option<ServerProfile>> {
        sqlx::query(GET_SERVER)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(map_server)
            .transpose()
    }

    pub async fn update_host(&self, id: &str, host: &str) -> AppResult<()> {
        if id.is_empty() || host.parse::<std::net::IpAddr>().is_err() {
            return Err(AppError::Validation(
                "服务器标识或已验证的新 IP 地址无效".into(),
            ));
        }
        let affected = sqlx::query(UPDATE_SERVER_HOST)
            .bind(host)
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        if affected != 1 {
            return Err(AppError::Validation("服务器不存在".into()));
        }
        Ok(())
    }

    pub async fn upsert_host_key(&self, key: &StoredHostKey) -> AppResult<()> {
        sqlx::query(UPSERT_HOST_KEY)
            .bind(&key.server_id)
            .bind(&key.algorithm)
            .bind(&key.fingerprint_sha256)
            .bind(&key.raw_key_base64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_host_key(&self, server_id: &str) -> AppResult<Option<StoredHostKey>> {
        sqlx::query(GET_HOST_KEY)
            .bind(server_id)
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(map_host_key)
            .transpose()
    }
}

fn map_server(row: &SqliteRow) -> AppResult<ServerProfile> {
    let port: i64 = row.try_get("port")?;
    let port = u16::try_from(port)
        .map_err(|_| AppError::Validation(format!("数据库中的服务器端口无效：{port}")))?;
    let auth_kind: String = row.try_get("auth_kind")?;

    Ok(ServerProfile {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        host: row.try_get("host")?,
        port,
        username: row.try_get("username")?,
        auth_kind: AuthKind::try_from(auth_kind.as_str())?,
        credential_id: row.try_get("credential_id")?,
    })
}

fn map_host_key(row: &SqliteRow) -> AppResult<StoredHostKey> {
    Ok(StoredHostKey {
        server_id: row.try_get("server_id")?,
        algorithm: row.try_get("algorithm")?,
        fingerprint_sha256: row.try_get("fingerprint_sha256")?,
        raw_key_base64: row.try_get("raw_key_base64")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::server::{AuthKind, ServerProfile, StoredHostKey},
        error::AppError,
    };

    #[sqlx::test(migrations = "./migrations")]
    async fn inserts_and_lists_server_without_secret_columns(pool: sqlx::SqlitePool) {
        let repository = ServerRepository::new(pool.clone());
        let server = ServerProfile::new(
            "网站服务器",
            "127.0.0.1",
            22,
            "tester",
            AuthKind::Password,
            "cred-1",
        );
        repository.insert(&server).await.unwrap();
        let listed = repository.list().await.unwrap();
        assert_eq!(listed, vec![server]);

        let secret_columns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('servers') WHERE lower(name) IN ('password','private_key','passphrase')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(secret_columns, 0);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn gets_server_and_upserts_its_host_key(pool: sqlx::SqlitePool) {
        let repository = ServerRepository::new(pool);
        let server = ServerProfile::new(
            "数据库服务器",
            "db.example.test",
            22,
            "operator",
            AuthKind::PrivateKey,
            "cred-2",
        );
        repository.insert(&server).await.unwrap();
        assert_eq!(
            repository.get(&server.id).await.unwrap(),
            Some(server.clone())
        );

        let mut key = StoredHostKey {
            server_id: server.id,
            algorithm: "ssh-ed25519".into(),
            fingerprint_sha256: "SHA256:first".into(),
            raw_key_base64: "Zmlyc3Q=".into(),
        };
        repository.upsert_host_key(&key).await.unwrap();
        assert_eq!(
            repository.get_host_key(&key.server_id).await.unwrap(),
            Some(key.clone())
        );

        key.fingerprint_sha256 = "SHA256:changed".into();
        key.raw_key_base64 = "Y2hhbmdlZA==".into();
        repository.upsert_host_key(&key).await.unwrap();
        assert_eq!(
            repository.get_host_key(&key.server_id).await.unwrap(),
            Some(key)
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn reports_a_corrupt_port_as_validation_error(pool: sqlx::SqlitePool) {
        let mut connection = pool.acquire().await.unwrap();
        sqlx::query("PRAGMA ignore_check_constraints = ON")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query(INSERT_SERVER)
            .bind("server-corrupt")
            .bind("损坏记录")
            .bind("127.0.0.1")
            .bind(70_000_i64)
            .bind("tester")
            .bind("password")
            .bind("cred-corrupt")
            .execute(&mut *connection)
            .await
            .unwrap();
        drop(connection);

        let error = ServerRepository::new(pool)
            .get("server-corrupt")
            .await
            .unwrap_err();
        assert!(matches!(error, AppError::Validation(_)));
    }
}
