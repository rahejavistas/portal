INSERT INTO permissions (code, label) VALUES
    ('documents.signatures.sign', 'View the signing queue and complete (sign) documents in it'),
    ('documents.signatures.manage', 'Cancel signature requirements and view signature status')
ON CONFLICT (code) DO NOTHING;

INSERT INTO designation_permissions (designation_id, permission_id)
SELECT d.designation_id, p.permission_id
FROM designations d, permissions p
WHERE d.code = 'super_admin' AND p.code = 'documents.signatures.manage'
ON CONFLICT DO NOTHING;
