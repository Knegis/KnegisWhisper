-- KNEG-368: where a note's transcript was made, as JSON
-- {"provider","name","onThisComputer","liveName"?}. NULL: not recorded
-- (notes from before this, typed notes, OpenWhispr and shared-folder imports).
ALTER TABLE notes ADD COLUMN transcribed_by TEXT;
