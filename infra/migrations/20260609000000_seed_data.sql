-- All seed data lives here, separate from schema.

INSERT INTO designations (code, label, is_system) VALUES
    ('super_admin', 'Super Admin', TRUE)
ON CONFLICT (code) DO NOTHING;

INSERT INTO permissions (code, label) VALUES
    ('users.manage', 'Edit/view residents and staff of the society'),
    ('users.view', 'View all residents'),
    ('users.roles.manage', 'Manage the designations of users'),
    ('users.roles.view', 'View the designations of users'),
    ('designations.manage', 'Create designations and manage their permissions and document category access'),
    ('documents.all.manage', 'View and manage all categories of documents'),
    ('documents.categories.manage', 'Create document categories'),
    ('process.view', 'View all existing processes'),
    ('process.reviewer', 'Review processes')
ON CONFLICT (code) DO NOTHING;

INSERT INTO designation_permissions (designation_id, permission_id)
SELECT d.designation_id, p.permission_id
FROM (VALUES
    ('super_admin', 'users.manage'),
    ('super_admin', 'users.view'),
    ('super_admin', 'users.roles.manage'),
    ('super_admin', 'users.roles.view'),
    ('super_admin', 'designations.manage'),
    ('super_admin', 'documents.all.manage'),
    ('super_admin', 'documents.categories.manage'),
    ('super_admin', 'process.view'),
    ('super_admin', 'process.reviewer')
) AS seed(designation_code, permission_code)
JOIN designations d ON d.code = seed.designation_code
JOIN permissions p ON p.code = seed.permission_code
ON CONFLICT DO NOTHING;

INSERT INTO document_categories (code, label, is_public_read, is_flat_scoped, is_system) VALUES
    ('PUBLIC', 'Public', TRUE, FALSE, TRUE),
    ('RESIDENT_SPECIFIC', 'Resident Specific', FALSE, TRUE, TRUE)
ON CONFLICT (code) DO NOTHING;
