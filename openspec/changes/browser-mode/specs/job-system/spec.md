# Spec Delta

## MODIFIED Requirements

### Requirement: Taskbar and dock progress
In the desktop app, the combined progress of all active jobs SHALL be mirrored to the Windows taskbar button and the macOS dock tile: nothing while idle, an indeterminate bar while sizes are unknown, the fraction while running, and the paused style when every active job is paused. A browser has no taskbar or dock; its progress SHALL follow "Progress in a browser", and no taskbar or dock on the server SHALL be touched.

#### Scenario: Everything paused
- **WHEN** every active job is paused in the desktop app
- **THEN** the taskbar button shows the progress in the paused style

#### Scenario: A browser
- **WHEN** the interface is served to a browser and a job runs
- **THEN** no taskbar or dock is touched on the server and the sidebar's progress card shows the job

## ADDED Requirements

### Requirement: Progress in a browser
In a browser, job progress SHALL arrive over the server's one stream with the same jobs, order, throttling and completion as the desktop app, and the sidebar's progress card and the Activity list SHALL work as they do in the desktop app. When the connection drops, the page SHALL reconnect and receive the events it missed; if the server cannot replay them, it SHALL send a reset and the page SHALL reload the job state. While no job is active the stream SHALL carry no progress messages. Every job SHALL record the session and client that started it, and the interface SHALL say "Started from another session" for a job started elsewhere.

#### Scenario: A job started in the browser
- **WHEN** the user starts a scan in a browser
- **THEN** the progress card shows it with the same counts and percentage as the desktop app, and it finishes with the same result

#### Scenario: Reconnect
- **WHEN** the connection drops while a scrape is at 3 of 25 and reconnects
- **THEN** the page first shows 3 of 25 and then continues from there

#### Scenario: Two sessions
- **WHEN** a job started in one tab is shown in another tab
- **THEN** it says "Started from another session", and cancelling it from the second tab asks first

#### Scenario: Idle
- **WHEN** no job is running
- **THEN** no progress message is sent to the browser
