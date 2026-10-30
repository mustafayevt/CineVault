# Spec Delta

## MODIFIED Requirements

### Requirement: The cv://img protocol
In the desktop app, images SHALL reach the interface only through `cv://img/<key>/<size>` (`grid`, `list`, `hero`, `full`), never over IPC, and only a parsed 32-character key and a size word SHALL reach the file system. A thumbnail SHALL be served as `image/webp` with `Cache-Control: public, max-age=31536000, immutable` and a strong ETag, 304 for a matching `If-None-Match`, 404 for an unknown key or missing source, and 405 for methods other than GET and HEAD. In a browser the server SHALL serve the same keys and sizes at `/img/<key>/<size>` under "Images in a browser", which keeps the key parsing and the image rules but uses a private cache because the content is behind a session.

#### Scenario: A repeated request
- **WHEN** the web view asks again for a grid thumbnail with the ETag it holds
- **THEN** the answer is 304 and no image is sent

#### Scenario: The browser asks for the same key
- **WHEN** a browser loads a poster card whose key the desktop app would request through `cv://img`
- **THEN** the browser requests `/img/<key>/grid` and receives the same thumbnail with a private cache header

## ADDED Requirements

### Requirement: Images in a browser
A browser served by CineVault SHALL receive every image from `/img/<key>/<size>` for the `grid`, `list`, `hero` and `full` size words. The answer SHALL have the content type, strong ETag, 304, 404 and 405 behaviour of the `cv://img` protocol. The `full` size SHALL answer with the original file when it is readable and the hero thumbnail otherwise. A size that cannot be made yet SHALL be answered by the largest cached size with `no-store`. The cache header SHALL be `private, max-age=31536000, immutable`, with no cross-origin allowance, and the route SHALL require a session. The route SHALL accept only a parsed 32-character key and a size word; a key with any other shape SHALL be a 404, not a file system read.

#### Scenario: Poster grid
- **WHEN** the library grid asks for 40 grid thumbnails
- **THEN** each request returns `image/webp` with a private one-year immutable cache header and a strong ETag

#### Scenario: Full backdrop
- **WHEN** a title page asks for `full` and the original fanart is readable
- **THEN** the original file is returned unchanged

#### Scenario: A size that is not cached yet
- **WHEN** the hero thumbnail is requested while only the grid size exists
- **THEN** the grid size is returned with `no-store`, and the real size is served on the next request

#### Scenario: Signed out
- **WHEN** an image is requested without a session
- **THEN** it is refused as signed out and no image is sent

#### Scenario: Unknown key
- **WHEN** a browser asks for `/img/not-a-key/grid`
- **THEN** the answer is 404 and no file is read

#### Scenario: Head request
- **WHEN** a browser sends HEAD to an image address
- **THEN** the headers are returned with no body

#### Scenario: Wrong method
- **WHEN** a browser sends POST to an image address
- **THEN** the answer is 405
