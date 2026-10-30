# Spec Delta

## ADDED Requirements

### Requirement: Browser access to the indexer service
When the interface is served to a browser, the managed indexer service still binds `127.0.0.1` on the server. The browser SHALL reach its pages through an authenticated server route, or the page SHALL show the server's address and port with a sentence saying it is reachable only from the server or a proxy. "Your Torznab instance" addresses SHALL be interpreted on the server, not on the client, and a loopback address SHALL be labelled as the server's loopback.

#### Scenario: Open the indexer manager
- **WHEN** the user chooses to open the indexer service from a browser
- **THEN** the manager opens through the authenticated route, or the page shows the server's address and says it is server-only

#### Scenario: Own Torznab on loopback
- **WHEN** the user enters `http://127.0.0.1:9117` as their Torznab address in a browser
- **THEN** the page says that address is the server's loopback and the test runs from the server
