-- D84: an automation closes its run's agent once the run finished and nobody took it over. On by
-- default; turned off, its sessions stay open.
ALTER TABLE automations ADD COLUMN close_on_success INTEGER NOT NULL DEFAULT 1 CHECK (close_on_success IN (0, 1));
