INSERT INTO designations (code, label, is_system) VALUES
    ('staff', 'Staff', TRUE)
ON CONFLICT (code) DO NOTHING;

INSERT INTO designation_permissions (designation_id, permission_id)
SELECT d.designation_id, p.permission_id
FROM (VALUES
    ('staff', 'users.manage'),
    ('staff', 'process.view'),
    ('staff', 'process.reviewer')
) AS seed(designation_code, permission_code)
JOIN designations d ON d.code = seed.designation_code
JOIN permissions p ON p.code = seed.permission_code
ON CONFLICT DO NOTHING;
