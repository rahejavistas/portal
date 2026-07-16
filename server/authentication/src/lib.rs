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

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, sqlx::FromRow)]
pub struct User {
    pub user_id: i32,
    pub email: String,
    pub is_active: bool,
    pub wing: String,
    pub flat_number: String,
    pub designations: Vec<String>,
}

impl User {
    /// Returns true if the user holds the given designation code (e.g. "staff", "committee").
    pub fn has_designation(&self, code: &str) -> bool {
        self.designations.iter().any(|d| d == code)
    }

    /// Returns true if the user is a Super Admin.
    pub fn is_admin(&self) -> bool {
        self.has_designation("super_admin")
    }

    /// Returns true if the user is a Committee member or Super Admin.
    pub fn is_committee_or_admin(&self) -> bool {
        self.is_admin() || self.has_designation("committee")
    }

    pub fn is_orphaned(&self) -> bool {
        !self.is_active
    }

    /// Returns the ID of the user's active flat, if any.
    pub async fn get_active_flat_id<'e, E: sqlx::Executor<'e, Database = sqlx::Postgres>>(
        &self,
        executor: E,
    ) -> Result<Option<i32>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT flat_id FROM flat_ownerships WHERE user_id = $1 AND is_active = TRUE LIMIT 1",
        )
        .bind(self.user_id)
        .fetch_optional(executor)
        .await
    }
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
    /// Creates a new instance of PermissionsManager.
    pub fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }

    /// Exposes the inner database connection pool.
    pub fn pool(&self) -> &Pool<Postgres> {
        &self.pool
    }

    /// Checks if a user is authenticated by their email and password.
    /// Returns an AuthResult indicating success, invalid credentials, or account lockout.
    pub async fn check_authenticated(
        &self,
        email: &str,
        password: &str,
    ) -> Result<AuthResult, sqlx::Error> {
        let query = format!(
            "SELECT password_hash, user_id, failed_login_attempts FROM {} WHERE user_id = (SELECT user_id FROM {} WHERE email = $1)",
            USER_CREDENTIALS_TABLE, USERS_TABLE
        );
        let row: Option<AuthRow> = sqlx::query_as(&query)
            .bind(email)
            .fetch_optional(&self.pool)
            .await?;

        if let Some(AuthRow {
            password_hash: Some(password_hash),
            user_id,
            failed_login_attempts: failed_attempts,
        }) = row
        {
            if failed_attempts >= 5 {
                return Ok(AuthResult::AccountLocked);
            }

            if verify_password(password, &password_hash) {
                // Reset failed attempts if they were > 0
                if failed_attempts > 0 {
                    sqlx::query(&format!(
                        "UPDATE {} SET failed_login_attempts = 0 WHERE user_id = $1",
                        USER_CREDENTIALS_TABLE
                    ))
                    .bind(user_id)
                    .execute(&self.pool)
                    .await?;
                }

                if let Some(user) = self.get_user_by_id(user_id).await? {
                    // Block orphaned users
                    if user.is_orphaned() {
                        return Ok(AuthResult::InvalidCredentials);
                    }
                    return Ok(AuthResult::Success(user));
                }
            } else {
                // Increment failed attempts on incorrect password
                sqlx::query(&format!("UPDATE {} SET failed_login_attempts = failed_login_attempts + 1 WHERE user_id = $1", USER_CREDENTIALS_TABLE))
                    .bind(user_id)
                    .execute(&self.pool)
                    .await?;
            }
        }

        Ok(AuthResult::InvalidCredentials)
    }

    /// Fetches a user's details, active roles, and flat assignments by their ID.
    pub async fn get_user_by_id(&self, user_id: i32) -> Result<Option<User>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        // 1. Fetch user base details
        let base_row: Option<(String, bool)> = sqlx::query_as(&format!(
            "SELECT email, is_active FROM {} WHERE user_id = $1",
            USERS_TABLE
        ))
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;

        let Some((email, is_active)) = base_row else {
            return Ok(None);
        };

        // 2. Fetch active designations
        let designations: Vec<String> = sqlx::query_scalar(
            "SELECT d.code FROM user_designations ud \
             JOIN designations d ON d.designation_id = ud.designation_id \
             WHERE ud.user_id = $1 AND ud.is_active = TRUE",
        )
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;

        // 3. Fetch active flat assignments
        let flats: Vec<(String, String)> = sqlx::query_as(
            "SELECT f.wing, f.flat_number \
             FROM flat_ownerships fo \
             JOIN flats f ON fo.flat_id = f.flat_id \
             WHERE fo.user_id = $1 AND fo.is_active = TRUE",
        )
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;

        // Format wing and flat numbers (if any)
        let (wing, flat_number) = if flats.is_empty() {
            ("TBD".to_string(), "TBD".to_string())
        } else {
            let wings: Vec<String> = flats.iter().map(|f| f.0.clone()).collect();
            let flat_nums: Vec<String> = flats.iter().map(|f| f.1.clone()).collect();
            (wings.join(", "), flat_nums.join(", "))
        };

        tx.commit().await?;

        Ok(Some(User {
            user_id,
            email,
            is_active,
            wing,
            flat_number,
            designations,
        }))
    }

    /// Creates a new user with their credentials in a transaction.
    pub async fn create_user_with_credentials(
        &self,
        email: &str,
        password_plain: Option<&str>,
        wing: &str,
        flat_number: &str,
    ) -> Result<User, String> {
        let password_hash = match password_plain {
            Some(pwd) => {
                validate_password_strength(pwd)?;
                Some(hash_password(pwd)?)
            }
            None => None,
        };

        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        // 1. Check if user already exists
        let existing_user_query = format!(
            "SELECT user_id, email FROM {} WHERE email = $1",
            USERS_TABLE
        );
        let existing_user_row: Option<(i32, String)> = sqlx::query_as(&existing_user_query)
            .bind(email)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let user_id = if let Some((id, _)) = existing_user_row {
            sqlx::query(&format!(
                "UPDATE {} SET is_active = TRUE, updated_at = NOW() WHERE user_id = $1",
                USERS_TABLE
            ))
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
            id
        } else {
            // Insert core user with email
            let user_insert_query = format!(
                "INSERT INTO {} (email) VALUES ($1) RETURNING user_id",
                USERS_TABLE
            );
            let user_row: (i32,) = sqlx::query_as(&user_insert_query)
                .bind(email)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;

            // Insert login credentials
            let credentials_query = format!(
                "INSERT INTO {} (user_id, password_hash) VALUES ($1, $2)",
                USER_CREDENTIALS_TABLE
            );
            sqlx::query(&credentials_query)
                .bind(user_row.0)
                .bind(&password_hash)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;

            user_row.0
        };

        // 2. Upsert the flat unit in flats table with wing
        let flat_insert_query = "INSERT INTO flats (wing, flat_number) VALUES ($1, $2) ON CONFLICT (wing, flat_number) DO UPDATE SET wing = EXCLUDED.wing RETURNING flat_id";
        let flat_row: (i32,) = sqlx::query_as(flat_insert_query)
            .bind(wing)
            .bind(flat_number)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        // 3. Map active ownership mapping
        let active_owner: Option<(i32,)> = sqlx::query_as(
            "SELECT user_id FROM flat_ownerships WHERE flat_id = $1 AND is_active = TRUE",
        )
        .bind(flat_row.0)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        if let Some((existing_owner_id,)) = active_owner {
            if existing_owner_id != user_id {
                return Err("Flat is already actively owned by another user".to_string());
            }
        } else {
            let ownership_insert_query =
                "INSERT INTO flat_ownerships (flat_id, user_id, is_active) VALUES ($1, $2, TRUE)";
            sqlx::query(ownership_insert_query)
                .bind(flat_row.0)
                .bind(user_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }

        tx.commit().await.map_err(|e| e.to_string())?;

        let user = User {
            user_id,
            email: email.to_string(),
            is_active: true,
            wing: wing.to_string(),
            flat_number: flat_number.to_string(),
            designations: Vec::new(),
        };

        Ok(user)
    }

    /// Creates a new user with no password (seeding), assigning them the given
    /// designation (e.g. "super_admin", "staff").
    pub async fn create_seeded_user(
        &self,
        email: &str,
        designation_code: &str,
    ) -> Result<User, String> {
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        // 1. Insert core user with email
        let user_insert_query = format!(
            "INSERT INTO {} (email) VALUES ($1) RETURNING user_id, email",
            USERS_TABLE
        );
        let user_row: (i32, String) = sqlx::query_as(&user_insert_query)
            .bind(email)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        // 2. Insert login credentials (no password_hash!)
        let credentials_query = format!(
            "INSERT INTO {} (user_id, password_hash) VALUES ($1, NULL)",
            USER_CREDENTIALS_TABLE
        );
        sqlx::query(&credentials_query)
            .bind(user_row.0)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        // 3. Assign the designation
        let designation_id: i32 =
            sqlx::query_scalar("SELECT designation_id FROM designations WHERE code = $1")
                .bind(designation_code)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Unknown designation: {}", designation_code))?;
        sqlx::query("INSERT INTO user_designations (user_id, designation_id) VALUES ($1, $2)")
            .bind(user_row.0)
            .bind(designation_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        tx.commit().await.map_err(|e| e.to_string())?;

        let user = User {
            user_id: user_row.0,
            email: user_row.1,
            is_active: true,
            wing: "TBD".to_string(),
            flat_number: "TBD".to_string(),
            designations: vec![designation_code.to_string()],
        };

        Ok(user)
    }

    /// Checks if a credential email exists and is not an orphaned account.
    pub async fn email_exists(&self, email: &str) -> Result<bool, sqlx::Error> {
        let query = format!("SELECT user_id FROM {} WHERE email = $1", USERS_TABLE);
        let row: Option<(i32,)> = sqlx::query_as(&query)
            .bind(email)
            .fetch_optional(&self.pool)
            .await?;

        if let Some((user_id,)) = row {
            if let Some(user) = self.get_user_by_id(user_id).await? {
                if user.is_orphaned() {
                    return Ok(false);
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Sets a secure password reset/setup token and expiration for an existing email.
    pub async fn create_reset_token(
        &self,
        email: &str,
        token: &str,
        expires_in_seconds: i64,
    ) -> Result<(), sqlx::Error> {
        let query = format!(
            "UPDATE {} SET reset_token = $1, reset_token_expires_at = NOW() + INTERVAL '1 second' * $2 \
             WHERE user_id = (SELECT user_id FROM {} WHERE email = $3)",
            USER_CREDENTIALS_TABLE, USERS_TABLE
        );
        sqlx::query(&query)
            .bind(token)
            .bind(expires_in_seconds)
            .bind(email)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Resets/sets up password for a user using a valid, non-expired token.
    /// Returns true if successful, and false if token is invalid or expired.
    ///
    /// Runs atomically — the token lookup and password update happen in a single
    /// transaction with a row lock to prevent concurrent token reuse.
    pub async fn reset_password_with_token(
        &self,
        token: &str,
        new_password_plain: &str,
    ) -> Result<bool, String> {
        // Validate strength
        validate_password_strength(new_password_plain)?;

        // Hash password outside the transaction (CPU-intensive, no DB needed)
        let new_hash = hash_password(new_password_plain)?;

        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        // 1. Verify token and lock the row to prevent concurrent reuse
        let check_query = format!(
            "SELECT uc.user_id FROM {} uc WHERE uc.reset_token = $1 AND uc.reset_token_expires_at > NOW() FOR UPDATE",
            USER_CREDENTIALS_TABLE
        );
        let row: Option<(i32,)> = sqlx::query_as(&check_query)
            .bind(token)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let cred_user_id = match row {
            Some((id,)) => id,
            None => return Ok(false), // Token invalid or expired — tx auto-rolls back on drop
        };

        // 2. Update password hash, clear token columns, and reset failed login attempts
        let update_query = format!(
            "UPDATE {} SET password_hash = $1, reset_token = NULL, reset_token_expires_at = NULL, failed_login_attempts = 0 WHERE user_id = $2",
            USER_CREDENTIALS_TABLE
        );
        sqlx::query(&update_query)
            .bind(new_hash)
            .bind(cred_user_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(true)
    }

    /// Sets the user's primary designation (committee) to the desired target state.
    /// - `Some("committee")` → deactivate the current committee designation, activate it.
    /// - `None` → deactivate (demote to plain Resident).
    ///
    /// Historical rows are preserved with is_active = FALSE for audit trail. "committee" is
    /// the only valid target here — super_admin is seed-only, and other designations (staff,
    /// treasurer, etc.) are managed via `assign_designation`/`revoke_designation`.
    pub async fn set_user_target_role(
        &self,
        user_id: i32,
        target_designation: Option<&str>,
    ) -> Result<(), String> {
        if let Some(code) = target_designation {
            if code != "committee" {
                return Err(format!("Invalid target role: {}", code));
            }
        }

        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        // 1. Deactivate the currently active committee designation, if any
        sqlx::query(
            "UPDATE user_designations SET is_active = FALSE \
             WHERE user_id = $1 AND is_active = TRUE \
               AND designation_id IN (SELECT designation_id FROM designations WHERE code = 'committee')",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

        // 2. If a target designation is specified, insert a fresh active mapping
        if let Some(code) = target_designation {
            let designation_id: i32 =
                sqlx::query_scalar("SELECT designation_id FROM designations WHERE code = $1")
                    .bind(code)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;
            sqlx::query("INSERT INTO user_designations (user_id, designation_id) VALUES ($1, $2)")
                .bind(user_id)
                .bind(designation_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }

        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Fetches all users from the users table, aggregating their active flat info and designations.
    pub async fn get_all_users(&self) -> Result<Vec<User>, sqlx::Error> {
        let query = format!(
            "WITH user_active_designations AS ( \
                 SELECT ud.user_id, ARRAY_AGG(d.code) as designations \
                 FROM user_designations ud \
                 JOIN designations d ON d.designation_id = ud.designation_id \
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
             ) \
             SELECT u.user_id, u.email, u.is_active, \
                    COALESCE(r.designations, ARRAY[]::TEXT[]) AS designations, \
                    COALESCE(f.wing, 'TBD') AS wing, \
                    COALESCE(f.flat_number, 'TBD') AS flat_number \
             FROM {} u \
             LEFT JOIN user_active_designations r ON u.user_id = r.user_id \
             LEFT JOIN user_flats f ON u.user_id = f.user_id \
             WHERE f.wing IS NOT NULL OR r.designations IS NOT NULL \
             ORDER BY u.user_id DESC",
            USERS_TABLE
        );
        sqlx::query_as::<_, User>(&query)
            .fetch_all(&self.pool)
            .await
    }

    /// Soft deletes a resident's flat mapping by setting is_active to FALSE.
    pub async fn revoke_flat_ownership(
        &self,
        user_id: i32,
        wing: &str,
        flat_number: &str,
    ) -> Result<(), String> {
        let flat_query = "SELECT flat_id FROM flats WHERE wing = $1 AND flat_number = $2";
        let flat_row: Option<(i32,)> = sqlx::query_as(flat_query)
            .bind(wing)
            .bind(flat_number)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?;

        if let Some((flat_id,)) = flat_row {
            sqlx::query(
                "UPDATE flat_ownerships SET is_active = FALSE, end_date = NOW() WHERE user_id = $1 AND flat_id = $2",
            )
            .bind(user_id)
            .bind(flat_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        }

        // Check if user is orphaned, if so mark inactive
        let u = self
            .get_user_by_id(user_id)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(user) = u {
            if user.wing == "TBD" && user.flat_number == "TBD" && user.designations.is_empty() {
                sqlx::query(&format!(
                    "UPDATE {} SET is_active = FALSE, updated_at = NOW() WHERE user_id = $1",
                    USERS_TABLE
                ))
                .bind(user_id)
                .execute(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct Designation {
    pub designation_id: i32,
    pub code: String,
    pub label: String,
    pub is_system: bool,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct Permission {
    pub permission_id: i32,
    pub code: String,
    pub label: String,
}

impl PermissionsManager {
    pub async fn list_designations(&self) -> Result<Vec<Designation>, sqlx::Error> {
        sqlx::query_as("SELECT designation_id, code, label, is_system FROM designations ORDER BY designation_id")
            .fetch_all(&self.pool)
            .await
    }

    pub async fn list_permissions(&self) -> Result<Vec<Permission>, sqlx::Error> {
        sqlx::query_as("SELECT permission_id, code, label FROM permissions ORDER BY permission_id")
            .fetch_all(&self.pool)
            .await
    }

    pub async fn get_designation_permissions(
        &self,
        designation_code: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT p.code FROM designation_permissions dp \
             JOIN permissions p ON p.permission_id = dp.permission_id \
             JOIN designations d ON d.designation_id = dp.designation_id \
             WHERE d.code = $1",
        )
        .bind(designation_code)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_user_designations(&self, user_id: i32) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT d.code FROM user_designations ud \
             JOIN designations d ON d.designation_id = ud.designation_id \
             WHERE ud.user_id = $1 AND ud.is_active = TRUE",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn user_has_permission(
        &self,
        user_id: i32,
        permission_code: &str,
    ) -> Result<bool, sqlx::Error> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_designations ud \
             JOIN designation_permissions dp ON dp.designation_id = ud.designation_id \
             JOIN permissions p ON p.permission_id = dp.permission_id \
             WHERE ud.user_id = $1 AND ud.is_active = TRUE AND p.code = $2",
        )
        .bind(user_id)
        .bind(permission_code)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    pub async fn assign_designation(
        &self,
        user_id: i32,
        designation_code: &str,
        assigned_by: Option<i32>,
    ) -> Result<(), String> {
        let designation_id: i32 =
            sqlx::query_scalar("SELECT designation_id FROM designations WHERE code = $1")
                .bind(designation_code)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("Unknown designation: {}", designation_code))?;

        sqlx::query(
            "INSERT INTO user_designations (user_id, designation_id, assigned_by) \
             VALUES ($1, $2, $3)",
        )
        .bind(user_id)
        .bind(designation_id)
        .bind(assigned_by)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub async fn revoke_designation(
        &self,
        user_id: i32,
        designation_code: &str,
    ) -> Result<(), String> {
        sqlx::query(
            "UPDATE user_designations SET is_active = FALSE \
             WHERE user_id = $1 AND is_active = TRUE \
               AND designation_id = (SELECT designation_id FROM designations WHERE code = $2)",
        )
        .bind(user_id)
        .bind(designation_code)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(())
    }
}

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
