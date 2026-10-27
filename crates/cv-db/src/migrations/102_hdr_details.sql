-- The Dolby Vision profile, mastering display and light levels of a video (HdrDetails as JSON).
-- Empty until the file's media info is read again.
ALTER TABLE media_file ADD COLUMN hdr_details TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(hdr_details));
