-- P4: the Chorus 1 import is gone (D50), and with it the columns that said where a project came from.

ALTER TABLE projects DROP COLUMN imported_from;
ALTER TABLE projects DROP COLUMN imported_id;
