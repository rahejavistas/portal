-- Per-user designations/permissions/flat/can_manage_documents, computed fresh each read.
CREATE OR REPLACE VIEW user_capabilities AS
SELECT
    u.user_id,
    u.email,
    u.is_active,
    COALESCE(r.designations, ARRAY[]::TEXT[]) AS designations,
    COALESCE(perm.permissions, ARRAY[]::TEXT[]) AS permissions,
    (COALESCE(perm.permissions, ARRAY[]::TEXT[]) && ARRAY['documents.all.manage']::TEXT[]
        OR dma.user_id IS NOT NULL) AS can_manage_documents,
    COALESCE(f.wing, 'TBD') AS wing,
    COALESCE(f.flat_number, 'TBD') AS flat_number
FROM users u
LEFT JOIN (
    SELECT ud.user_id, ARRAY_AGG(d.code) AS designations
    FROM user_designations ud
    JOIN designations d ON d.designation_id = ud.designation_id
    WHERE ud.is_active = TRUE
    GROUP BY ud.user_id
) r ON u.user_id = r.user_id
LEFT JOIN (
    SELECT ud.user_id, ARRAY_AGG(DISTINCT p.code::TEXT) AS permissions
    FROM user_designations ud
    JOIN designation_permissions dp ON dp.designation_id = ud.designation_id
    JOIN permissions p ON p.permission_id = dp.permission_id
    WHERE ud.is_active = TRUE
    GROUP BY ud.user_id
) perm ON u.user_id = perm.user_id
LEFT JOIN (
    SELECT fo.user_id, string_agg(fl.wing, ', ') AS wing, string_agg(fl.flat_number, ', ') AS flat_number
    FROM flat_ownerships fo
    JOIN flats fl ON fl.flat_id = fo.flat_id
    WHERE fo.is_active = TRUE
    GROUP BY fo.user_id
) f ON u.user_id = f.user_id
LEFT JOIN (
    SELECT DISTINCT ud.user_id
    FROM user_designations ud
    JOIN document_category_designations dcd ON dcd.designation_id = ud.designation_id
    WHERE ud.is_active = TRUE AND dcd.can_manage = TRUE
) dma ON u.user_id = dma.user_id;

-- Trigger-maintained cache of user_capabilities, so the per-request auth lookup is a
-- zero-join PK read instead of recomputing the view's joins on every request.
CREATE TABLE IF NOT EXISTS user_effective_access (
    user_id INT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    designations TEXT[] NOT NULL DEFAULT '{}',
    permissions TEXT[] NOT NULL DEFAULT '{}',
    can_manage_documents BOOLEAN NOT NULL DEFAULT FALSE,
    wing VARCHAR(255) NOT NULL DEFAULT 'TBD',
    flat_number VARCHAR(255) NOT NULL DEFAULT 'TBD',
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE OR REPLACE FUNCTION refresh_user_effective_access(p_user_id INT) RETURNS VOID AS $$
BEGIN
    INSERT INTO user_effective_access (user_id, designations, permissions, can_manage_documents, wing, flat_number, updated_at)
    SELECT user_id, designations, permissions, can_manage_documents, wing, flat_number, NOW()
    FROM user_capabilities WHERE user_id = p_user_id
    ON CONFLICT (user_id) DO UPDATE SET
        designations = EXCLUDED.designations,
        permissions = EXCLUDED.permissions,
        can_manage_documents = EXCLUDED.can_manage_documents,
        wing = EXCLUDED.wing,
        flat_number = EXCLUDED.flat_number,
        updated_at = NOW();
END;
$$ LANGUAGE plpgsql;

-- Refreshes one user's cache row on their own designation/flat changes.
CREATE OR REPLACE FUNCTION trg_refresh_single_user() RETURNS TRIGGER AS $$
BEGIN
    PERFORM refresh_user_effective_access(COALESCE(NEW.user_id, OLD.user_id));
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_user_designations_refresh
AFTER INSERT OR UPDATE OR DELETE ON user_designations
FOR EACH ROW EXECUTE FUNCTION trg_refresh_single_user();

CREATE TRIGGER trg_flat_ownerships_refresh
AFTER INSERT OR UPDATE OR DELETE ON flat_ownerships
FOR EACH ROW EXECUTE FUNCTION trg_refresh_single_user();

-- Refreshes every user holding a designation whose permissions/category-access/code changed.
CREATE OR REPLACE FUNCTION trg_refresh_users_by_designation() RETURNS TRIGGER AS $$
DECLARE
    affected_designation_id INT;
BEGIN
    affected_designation_id := COALESCE(NEW.designation_id, OLD.designation_id);
    PERFORM refresh_user_effective_access(ud.user_id)
    FROM user_designations ud
    WHERE ud.designation_id = affected_designation_id AND ud.is_active = TRUE;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_designation_permissions_refresh
AFTER INSERT OR UPDATE OR DELETE ON designation_permissions
FOR EACH ROW EXECUTE FUNCTION trg_refresh_users_by_designation();

CREATE TRIGGER trg_document_category_designations_refresh
AFTER INSERT OR UPDATE OR DELETE ON document_category_designations
FOR EACH ROW EXECUTE FUNCTION trg_refresh_users_by_designation();

CREATE TRIGGER trg_designations_code_refresh
AFTER UPDATE OF code ON designations
FOR EACH ROW EXECUTE FUNCTION trg_refresh_users_by_designation();
