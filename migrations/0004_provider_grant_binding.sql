-- Bind in-flight OAuth codes to their original app; pre-upgrade flows must restart.
ALTER TABLE connection_flows ADD COLUMN encrypted_app text;
