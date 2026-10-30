-- Reads the media info of some videos again, now that it is mapped better: Blu-ray discs, disc
-- images and Blu-ray stream files (the Dolby Vision enhancement layer of an Ultra HD Blu-ray, the
-- track languages in the clip information), videos with an E-AC-3 track stored as AC3 (an AC3
-- track never has more than six channels), with a DTS:X track whose height speakers were counted
-- as channels, and interlaced videos (the frame rate of video stored as separate fields). An empty
-- container format marks a video as not read; the stored details stay until the new read replaces
-- them, and nothing else is touched.
UPDATE media_file
   SET container_format = ''
 WHERE kind = 'video'
   AND container_format <> ''
   AND (lower(container_format) IN ('m2ts', 'mts', 'ssif', 'blu-ray video')
        OR lower(filename) IN ('bdmv', 'index.bdmv')
        OR lower(filename) LIKE '%.m2ts'
        OR lower(filename) LIKE '%.mts'
        OR lower(filename) LIKE '%.iso'
        OR exact_video_format LIKE '%i'
        OR id IN (SELECT media_file_id FROM stream
                   WHERE kind = 'audio'
                     AND ((codec = 'DTS-X' AND channels > 8)
                          OR (codec = 'AC3' AND channels > 6))));
