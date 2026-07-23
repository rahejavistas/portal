use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use sqlx::{Pool, Postgres};

pub const USERS_TABLE: &str = "users";
pub const USER_CREDENTIALS_TABLE: &str = "user_credentials";

pub enum AuthResult {
    Success(User),
    InvalidCredentials,
    AccountLocked,
}

/// All permission codes, compile-time checked instead of raw strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    UsersManage,
    UsersView,
    UsersRolesManage,
    UsersRolesView,
    DesignationsManage,
    DocumentsAllManage,
    DocumentsCategoriesManage,
    ProcessView,
    ProcessReviewer,
    ProcessTemplatesManage,
}

impl Permission {
    pub fn code(&self) -> &'static str {
        match self {
            Permission::UsersManage => "users.manage",
            Permission::UsersView => "users.view",
            Permission::UsersRolesManage => "users.roles.manage",
            Permission::UsersRolesView => "users.roles.view",
            Permission::DesignationsManage => "designations.manage",
            Permission::DocumentsAllManage => "documents.all.manage",
            Permission::DocumentsCategoriesManage => "documents.categories.manage",
            Permission::ProcessView => "process.view",
            Permission::ProcessReviewer => "process.reviewer",
            Permission::ProcessTemplatesManage => "process.templates.manage",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "users.manage" => Some(Permission::UsersManage),
            "users.view" => Some(Permission::UsersView),
            "users.roles.manage" => Some(Permission::UsersRolesManage),
            "users.roles.view" => Some(Permission::UsersRolesView),
            "designations.manage" => Some(Permission::DesignationsManage),
            "documents.all.manage" => Some(Permission::DocumentsAllManage),
            "documents.categories.manage" => Some(Permission::DocumentsCategoriesManage),
            "process.view" => Some(Permission::ProcessView),
            "process.reviewer" => Some(Permission::ProcessReviewer),
            "process.templates.manage" => Some(Permission::ProcessTemplatesManage),
            _ => None,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct User {
    pub user_id: i32,
    pub email: String,
    pub is_active: bool,
    pub wing: String,
    pub flat_number: String,
    pub designations: Vec<String>,
    /// Never serialized — internal only, use `has_permission`.
    #[serde(skip)]
    pub permissions: Vec<String>,
    /// Frontend capability flags, so the UI never sees raw permission codes. Flattened
    /// into the JSON object, so the wire format is unaffected by this being a
    /// sub-struct (`user.can_manage_roles`, not `user.capabilities.can_manage_roles`).
    #[serde(flatten)]
    pub capabilities: Capabilities,
}

/// Frontend-facing capability flags, all derived from `permissions` (see
/// `derived_capabilities`) except `can_manage_documents`, which also needs a
/// per-category grant lookup a static permission list can't answer.
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, Copy, Default)]
pub struct Capabilities {
    pub can_manage_roles: bool,
    pub can_manage_designations: bool,
    pub can_manage_categories: bool,
    pub can_manage_users: bool,
    pub can_view_users: bool,
    /// Read-only view of who holds which designation (no assign/revoke rights).
    pub can_view_roles: bool,
    pub can_review_process: bool,
    pub can_manage_documents: bool,
    /// Can browse every resident's process (read-only unless `can_review_process`).
    pub can_view_all_processes: bool,
    /// Can create/edit process templates (the versioned builder) and activate versions.
    pub can_manage_process_templates: bool,
}

impl User {
    /// Returns true if the user holds the given designation code (e.g. "staff", "committee").
    pub fn has_designation(&self, code: &str) -> bool {
        self.designations.iter().any(|d| d == code)
    }

    /// The single place authorization checks go through: does this user hold `perm`?
    pub fn has_permission(&self, perm: Permission) -> bool {
        self.permissions.iter().any(|p| p == perm.code())
    }

    /// Returns true if the user is a Super Admin.
    pub fn is_admin(&self) -> bool {
        self.has_designation("super_admin")
    }

    pub fn is_orphaned(&self) -> bool {
        !self.is_active
    }

    /// IDs of every flat the user actively owns. A user can own more than one flat, so
    /// callers must not assume a single result (there's no DB constraint limiting a user
    /// to one active ownership — only one active owner per flat).
    pub async fn get_active_flat_ids<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        &self,
        executor: E,
    ) -> Result<Vec<i32>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT flat_id FROM flat_ownerships WHERE user_id = $1 AND is_active = TRUE",
        )
        .bind(self.user_id)
        .fetch_all(executor)
        .await
    }
}

/// Raw parts for building a `User` — the one place every capability flag gets derived.
/// Also directly `FromRow`-fetchable, so `get_all_users` can select these columns and
/// skip computing capability flags in SQL entirely.
#[derive(sqlx::FromRow)]
struct RawUser {
    user_id: i32,
    email: String,
    is_active: bool,
    wing: String,
    flat_number: String,
    designations: Vec<String>,
    permissions: Vec<String>,
    can_manage_documents: bool,
}

fn assemble_user(raw: RawUser) -> User {
    let capabilities = Capabilities {
        can_manage_documents: raw.can_manage_documents,
        ..derived_capabilities(&raw.permissions)
    };
    User {
        user_id: raw.user_id,
        email: raw.email,
        is_active: raw.is_active,
        wing: raw.wing,
        flat_number: raw.flat_number,
        designations: raw.designations,
        permissions: raw.permissions,
        capabilities,
    }
}

/// Derives every `Capabilities` flag from a resolved permission-code list, except
/// `can_manage_documents` (left at its `Default`) — that one needs a DB lookup.
fn derived_capabilities(permissions: &[String]) -> Capabilities {
    let has = |p: Permission| permissions.iter().any(|c| c == p.code());
    let can_manage_roles = has(Permission::UsersRolesManage);
    let can_review_process = has(Permission::ProcessReviewer);
    Capabilities {
        can_manage_roles,
        can_manage_designations: has(Permission::DesignationsManage),
        can_manage_categories: has(Permission::DocumentsCategoriesManage),
        can_manage_users: has(Permission::UsersManage),
        can_view_users: has(Permission::UsersManage) || has(Permission::UsersView),
        can_view_roles: can_manage_roles || has(Permission::UsersRolesView),
        can_review_process,
        can_manage_documents: false,
        can_view_all_processes: can_review_process || has(Permission::ProcessView),
        can_manage_process_templates: has(Permission::ProcessTemplatesManage),
    }
}

/// Permission codes granted to a user via their active designations.
async fn fetch_user_permissions<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
    executor: E,
    user_id: i32,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT DISTINCT p.code FROM user_designations ud \
         JOIN designation_permissions dp ON dp.designation_id = ud.designation_id \
         JOIN permissions p ON p.permission_id = dp.permission_id \
         WHERE ud.user_id = $1 AND ud.is_active = TRUE",
    )
    .bind(user_id)
    .fetch_all(executor)
    .await
}

/// True if the user manages documents in at least one category, either via
/// `documents.all.manage` or a per-category `can_manage` grant on one of their designations.
async fn fetch_can_manage_documents<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
    executor: E,
    user_id: i32,
    permissions: &[String],
) -> Result<bool, sqlx::Error> {
    if permissions
        .iter()
        .any(|c| c == Permission::DocumentsAllManage.code())
    {
        return Ok(true);
    }
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM document_category_designations dcd \
         JOIN user_designations ud ON ud.designation_id = dcd.designation_id \
         WHERE ud.user_id = $1 AND ud.is_active = TRUE AND dcd.can_manage = TRUE)",
    )
    .bind(user_id)
    .fetch_one(executor)
    .await
}

/// Appends one row to the permission-change audit log. Shared across crates (document
/// category access lives in `file_server`, not here) since both write to the same DB.
pub async fn log_permission_change(
    pool: &Pool<Postgres>,
    description: impl Into<String>,
    done_by: Option<i32>,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO permission_log (description, done_by) VALUES ($1, $2)")
        .bind(description.into())
        .bind(done_by)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct AuthRow {
    password_hash: Option<String>,
    user_id: i32,
    failed_login_attempts: i32,
}

pub struct PermissionsManager {
    pool: Pool<Postgres>,
}

impl PermissionsManager {
    pub fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &Pool<Postgres> {
        &self.pool
    }

    pub async fn check_authenticated(
        &self,
        email: &str,
        password: &str,
    ) -> Result<AuthResult, sqlx::Error> {
        let row: Option<AuthRow> = sqlx::query_as(
            "SELECT password_hash, user_id, failed_login_attempts FROM user_credentials \
             WHERE user_id = (SELECT user_id FROM users WHERE email = $1)",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;

        let Some(AuthRow {
            password_hash: Some(password_hash),
            user_id,
            failed_login_attempts: failed_attempts,
        }) = row
        else {
            return Ok(AuthResult::InvalidCredentials);
        };

        if failed_attempts >= 5 {
            return Ok(AuthResult::AccountLocked);
        }

        if !verify_password(password, &password_hash) {
            sqlx::query(
                "UPDATE user_credentials SET failed_login_attempts = failed_login_attempts + 1 \
                 WHERE user_id = $1",
            )
            .bind(user_id)
            .execute(&self.pool)
            .await?;
            return Ok(AuthResult::InvalidCredentials);
        }

        if failed_attempts > 0 {
            sqlx::query("UPDATE user_credentials SET failed_login_attempts = 0 WHERE user_id = $1")
                .bind(user_id)
                .execute(&self.pool)
                .await?;
        }

        match self.get_user_by_id(user_id).await? {
            Some(user) if !user.is_orphaned() => Ok(AuthResult::Success(user)),
            _ => Ok(AuthResult::InvalidCredentials),
        }
    }

    /// The per-request hot path (every authenticated request re-resolves its session
    /// user from a bare user_id). One query, one round trip, one consistent snapshot —
    /// not a transaction juggling five separate statements.
    pub async fn get_user_by_id(&self, user_id: i32) -> Result<Option<User>, sqlx::Error> {
        let row: Option<RawUser> = sqlx::query_as(
            "WITH ud AS ( \
                 SELECT designation_id FROM user_designations \
                 WHERE user_id = $1 AND is_active = TRUE \
             ), \
             desigs AS ( \
                 SELECT ARRAY_AGG(d.code) AS codes FROM designations d \
                 WHERE d.designation_id IN (SELECT designation_id FROM ud) \
             ), \
             perms AS ( \
                 SELECT ARRAY_AGG(DISTINCT p.code::TEXT) AS codes FROM permissions p \
                 JOIN designation_permissions dp ON dp.permission_id = p.permission_id \
                 WHERE dp.designation_id IN (SELECT designation_id FROM ud) \
             ), \
             flat AS ( \
                 SELECT string_agg(f.wing, ', ') AS wing, string_agg(f.flat_number, ', ') AS flat_number \
                 FROM flat_ownerships fo JOIN flats f ON f.flat_id = fo.flat_id \
                 WHERE fo.user_id = $1 AND fo.is_active = TRUE \
             ), \
             doc_manage AS ( \
                 SELECT 1 FROM document_category_designations dcd \
                 WHERE dcd.can_manage = TRUE AND dcd.designation_id IN (SELECT designation_id FROM ud) \
                 LIMIT 1 \
             ) \
             SELECT u.user_id, u.email, u.is_active, \
                    COALESCE(desigs.codes, ARRAY[]::TEXT[]) AS designations, \
                    COALESCE(perms.codes, ARRAY[]::TEXT[]) AS permissions, \
                    (COALESCE(perms.codes, ARRAY[]::TEXT[]) && ARRAY['documents.all.manage']::TEXT[] \
                        OR EXISTS(SELECT 1 FROM doc_manage)) AS can_manage_documents, \
                    COALESCE(flat.wing, 'TBD') AS wing, \
                    COALESCE(flat.flat_number, 'TBD') AS flat_number \
             FROM users u, desigs, perms, flat \
             WHERE u.user_id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(assemble_user))
    }

    pub async fn create_user_with_credentials(
        &self,
        email: &str,
        password_plain: Option<&str>,
        wing: &str,
        flat_number: &str,
        created_by: Option<i32>,
    ) -> Result<User, String> {
        let password_hash = match password_plain {
            Some(pwd) => {
                validate_password_strength(pwd)?;
                Some(hash_password(pwd)?)
            }
            None => None,
        };

        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        let existing_user_row: Option<(i32,)> =
            sqlx::query_as("SELECT user_id FROM users WHERE email = $1")
                .bind(email)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;

        let user_id = if let Some((id,)) = existing_user_row {
            sqlx::query("UPDATE users SET is_active = TRUE, updated_at = NOW() WHERE user_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            id
        } else {
            let (new_user_id,): (i32,) = sqlx::query_as(
                "INSERT INTO users (email, created_by) VALUES ($1, $2) RETURNING user_id",
            )
            .bind(email)
            .bind(created_by)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

            sqlx::query("INSERT INTO user_credentials (user_id, password_hash) VALUES ($1, $2)")
                .bind(new_user_id)
                .bind(&password_hash)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;

            new_user_id
        };

        // Upsert the flat unit, then map ownership — fails if another user already
        // actively owns it.
        let (flat_id,): (i32,) = sqlx::query_as(
            "INSERT INTO flats (wing, flat_number) VALUES ($1, $2) \
             ON CONFLICT (wing, flat_number) DO UPDATE SET wing = EXCLUDED.wing \
             RETURNING flat_id",
        )
        .bind(wing)
        .bind(flat_number)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        let active_owner: Option<(i32,)> = sqlx::query_as(
            "SELECT user_id FROM flat_ownerships WHERE flat_id = $1 AND is_active = TRUE",
        )
        .bind(flat_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        match active_owner {
            Some((existing_owner_id,)) if existing_owner_id != user_id => {
                return Err("Flat is already actively owned by another user".to_string());
            }
            Some(_) => {}
            None => {
                sqlx::query(
                    "INSERT INTO flat_ownerships (flat_id, user_id, is_active) VALUES ($1, $2, TRUE)",
                )
                .bind(flat_id)
                .bind(user_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            }
        }

        tx.commit().await.map_err(|e| e.to_string())?;

        Ok(assemble_user(RawUser {
            user_id,
            email: email.to_string(),
            is_active: true,
            wing: wing.to_string(),
            flat_number: flat_number.to_string(),
            designations: Vec::new(),
            permissions: Vec::new(),
            can_manage_documents: false,
        }))
    }

    /// Staff account — no flat ownership, assigned the `staff` designation. Password is
    /// set via the same forgot-password flow as resident accounts.
    pub async fn create_staff_user(
        &self,
        email: &str,
        created_by: Option<i32>,
    ) -> Result<User, String> {
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        let existing_user_row: Option<(i32,)> =
            sqlx::query_as("SELECT user_id FROM users WHERE email = $1")
                .bind(email)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;

        let user_id = if let Some((id,)) = existing_user_row {
            sqlx::query("UPDATE users SET is_active = TRUE, updated_at = NOW() WHERE user_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            id
        } else {
            let (new_user_id,): (i32,) = sqlx::query_as(
                "INSERT INTO users (email, created_by) VALUES ($1, $2) RETURNING user_id",
            )
            .bind(email)
            .bind(created_by)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

            sqlx::query("INSERT INTO user_credentials (user_id, password_hash) VALUES ($1, NULL)")
                .bind(new_user_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;

            new_user_id
        };

        tx.commit().await.map_err(|e| e.to_string())?;

        self.assign_designation(user_id, "staff", created_by)
            .await?;

        let permissions = fetch_user_permissions(&self.pool, user_id)
            .await
            .map_err(|e| e.to_string())?;
        let can_manage_documents = fetch_can_manage_documents(&self.pool, user_id, &permissions)
            .await
            .map_err(|e| e.to_string())?;

        Ok(assemble_user(RawUser {
            user_id,
            email: email.to_string(),
            is_active: true,
            wing: "N/A".to_string(),
            flat_number: "N/A".to_string(),
            designations: vec!["staff".to_string()],
            permissions,
            can_manage_documents,
        }))
    }

    /// Passwordless seed account, assigned the given designation (e.g. "super_admin").
    pub async fn create_seeded_user(
        &self,
        email: &str,
        designation_code: &str,
    ) -> Result<User, String> {
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        let (user_id, email): (i32, String) =
            sqlx::query_as("INSERT INTO users (email) VALUES ($1) RETURNING user_id, email")
                .bind(email)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;

        sqlx::query("INSERT INTO user_credentials (user_id, password_hash) VALUES ($1, NULL)")
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let designation_id: i32 =
            sqlx::query_scalar("SELECT designation_id FROM designations WHERE code = $1")
                .bind(designation_code)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Unknown designation: {}", designation_code))?;
        sqlx::query("INSERT INTO user_designations (user_id, designation_id) VALUES ($1, $2)")
            .bind(user_id)
            .bind(designation_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let permissions = fetch_user_permissions(&mut *tx, user_id)
            .await
            .map_err(|e| e.to_string())?;
        let can_manage_documents = fetch_can_manage_documents(&mut *tx, user_id, &permissions)
            .await
            .map_err(|e| e.to_string())?;

        tx.commit().await.map_err(|e| e.to_string())?;

        Ok(assemble_user(RawUser {
            user_id,
            email,
            is_active: true,
            wing: "TBD".to_string(),
            flat_number: "TBD".to_string(),
            designations: vec![designation_code.to_string()],
            permissions,
            can_manage_documents,
        }))
    }

    pub async fn email_exists(&self, email: &str) -> Result<bool, sqlx::Error> {
        let row: Option<(i32,)> = sqlx::query_as("SELECT user_id FROM users WHERE email = $1")
            .bind(email)
            .fetch_optional(&self.pool)
            .await?;

        let Some((user_id,)) = row else {
            return Ok(false);
        };
        match self.get_user_by_id(user_id).await? {
            Some(user) => Ok(!user.is_orphaned()),
            None => Ok(false),
        }
    }

    /// Sets a secure password reset/setup token and expiration for an existing email.
    pub async fn create_reset_token(
        &self,
        email: &str,
        token: &str,
        expires_in_seconds: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE user_credentials SET reset_token = $1, \
             reset_token_expires_at = NOW() + INTERVAL '1 second' * $2 \
             WHERE user_id = (SELECT user_id FROM users WHERE email = $3)",
        )
        .bind(token)
        .bind(expires_in_seconds)
        .bind(email)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Returns false if the token is invalid/expired. Row-locked to prevent concurrent reuse.
    pub async fn reset_password_with_token(
        &self,
        token: &str,
        new_password_plain: &str,
    ) -> Result<bool, String> {
        validate_password_strength(new_password_plain)?;
        // Hash outside the transaction — CPU-intensive, no DB needed.
        let new_hash = hash_password(new_password_plain)?;

        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        let row: Option<(i32,)> = sqlx::query_as(
            "SELECT user_id FROM user_credentials \
             WHERE reset_token = $1 AND reset_token_expires_at > NOW() FOR UPDATE",
        )
        .bind(token)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        let Some((cred_user_id,)) = row else {
            // Token invalid or expired — tx auto-rolls back on drop.
            return Ok(false);
        };

        sqlx::query(
            "UPDATE user_credentials SET password_hash = $1, reset_token = NULL, \
             reset_token_expires_at = NULL, failed_login_attempts = 0 WHERE user_id = $2",
        )
        .bind(new_hash)
        .bind(cred_user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(true)
    }

    /// Fetches all users from the users table, aggregating their active flat info, designations,
    /// and resolved permissions.
    pub async fn get_all_users(&self) -> Result<Vec<User>, sqlx::Error> {
        sqlx::query_as(
            "WITH user_active_designations AS ( \
                 SELECT ud.user_id, ARRAY_AGG(d.code) as designations \
                 FROM user_designations ud \
                 JOIN designations d ON d.designation_id = ud.designation_id \
                 WHERE ud.is_active = TRUE \
                 GROUP BY ud.user_id \
             ), \
             user_permissions AS ( \
                 SELECT ud.user_id, ARRAY_AGG(DISTINCT p.code::TEXT) as permissions \
                 FROM user_designations ud \
                 JOIN designation_permissions dp ON dp.designation_id = ud.designation_id \
                 JOIN permissions p ON p.permission_id = dp.permission_id \
                 WHERE ud.is_active = TRUE \
                 GROUP BY ud.user_id \
             ), \
             user_flats AS ( \
                 SELECT fo.user_id, \
                        string_agg(f.wing, ', ') as wing, \
                        string_agg(f.flat_number, ', ') as flat_number \
                 FROM flat_ownerships fo \
                 JOIN flats f ON fo.flat_id = f.flat_id \
                 WHERE fo.is_active = TRUE \
                 GROUP BY fo.user_id \
             ), \
             user_document_manage_access AS ( \
                 SELECT DISTINCT ud.user_id \
                 FROM user_designations ud \
                 JOIN document_category_designations dcd ON dcd.designation_id = ud.designation_id \
                 WHERE ud.is_active = TRUE AND dcd.can_manage = TRUE \
             ) \
             SELECT u.user_id, u.email, u.is_active, \
                    COALESCE(r.designations, ARRAY[]::TEXT[]) AS designations, \
                    COALESCE(perm.permissions, ARRAY[]::TEXT[]) AS permissions, \
                    (COALESCE(perm.permissions, ARRAY[]::TEXT[]) && ARRAY['documents.all.manage']::TEXT[] \
                        OR dma.user_id IS NOT NULL) AS can_manage_documents, \
                    COALESCE(f.wing, 'TBD') AS wing, \
                    COALESCE(f.flat_number, 'TBD') AS flat_number \
             FROM users u \
             LEFT JOIN user_active_designations r ON u.user_id = r.user_id \
             LEFT JOIN user_permissions perm ON u.user_id = perm.user_id \
             LEFT JOIN user_flats f ON u.user_id = f.user_id \
             LEFT JOIN user_document_manage_access dma ON u.user_id = dma.user_id \
             WHERE f.wing IS NOT NULL OR r.designations IS NOT NULL \
             ORDER BY u.user_id DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map(|rows: Vec<RawUser>| rows.into_iter().map(assemble_user).collect())
    }

    /// Soft deletes a resident's flat mapping by setting is_active to FALSE.
    pub async fn revoke_flat_ownership(
        &self,
        user_id: i32,
        wing: &str,
        flat_number: &str,
    ) -> Result<(), String> {
        let flat_row: Option<(i32,)> =
            sqlx::query_as("SELECT flat_id FROM flats WHERE wing = $1 AND flat_number = $2")
                .bind(wing)
                .bind(flat_number)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| e.to_string())?;

        if let Some((flat_id,)) = flat_row {
            sqlx::query(
                "UPDATE flat_ownerships SET is_active = FALSE, end_date = NOW() \
                 WHERE user_id = $1 AND flat_id = $2",
            )
            .bind(user_id)
            .bind(flat_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        }

        // If that was their last flat and they hold no designations, they're orphaned.
        if let Some(user) = self
            .get_user_by_id(user_id)
            .await
            .map_err(|e| e.to_string())?
        {
            if user.wing == "TBD" && user.flat_number == "TBD" && user.designations.is_empty() {
                sqlx::query(
                    "UPDATE users SET is_active = FALSE, updated_at = NOW() WHERE user_id = $1",
                )
                .bind(user_id)
                .execute(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
            }
        }

        Ok(())
    }

    /// Emails of active owners of a flat, for previewing who'll gain access before
    /// mapping a document to it. Empty if the flat doesn't exist or has no active owner.
    pub async fn active_owner_emails_for_flat(
        &self,
        wing: &str,
        flat_number: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT u.email FROM flat_ownerships fo \
             JOIN flats f ON fo.flat_id = f.flat_id \
             JOIN users u ON fo.user_id = u.user_id \
             WHERE f.wing = $1 AND f.flat_number = $2 AND fo.is_active = TRUE \
             ORDER BY u.email",
        )
        .bind(wing)
        .bind(flat_number)
        .fetch_all(&self.pool)
        .await
    }
}

mod designations;
pub use designations::{Designation, PermissionRow};

/// Validates that a password is strong enough (>= 8 characters, at least 1 number)
pub fn validate_password_strength(password: &str) -> Result<(), &'static str> {
    if password.len() < 8 {
        return Err("Password must be at least 8 characters long");
    }
    if !password.chars().any(|c| c.is_ascii_digit()) {
        return Err("Password must contain at least one number");
    }
    Ok(())
}

/// Hashes a password using Argon2id.
pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

/// Verifies a password against an Argon2id hash.
pub fn verify_password(password: &str, hash: &str) -> bool {
    if let Ok(parsed_hash) = PasswordHash::new(hash) {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok()
    } else {
        false
    }
}
