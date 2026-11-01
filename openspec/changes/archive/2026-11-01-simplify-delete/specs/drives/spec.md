## REMOVED Requirements

### Requirement: Empty a drive's trash
**Reason**: Emptying no longer asks for a typed word; replaced by "Empty trash folders with a Yes or No".
**Migration**: None; the same Empty… and Empty all trash folders… entries ask Yes or No.

## ADDED Requirements

### Requirement: Empty trash folders with a Yes or No
A drive card with trash SHALL offer "Empty…", and the Drives page menu "Empty all trash folders…". Both SHALL open a Yes or No confirmation listing each trash-folder batch and bin item with its size, with the buttons "Cancel" and "Empty for good". Confirming SHALL erase only top entries of a `.deletedByCineVault` folder (never the custom folder's other contents; links removed, never followed) and only the recorded bin items, then refresh the drives and History.

#### Scenario: Empty D:
- **WHEN** the user chooses Empty… on D: and confirms with "Empty for good"
- **THEN** D:'s trash folder batches and the items CineVault sent to the Recycle Bin from D: are erased, the toast says "Emptied the trash: <size> freed", and the delete entries of those files show why their Undo is off

#### Scenario: Cancelled
- **WHEN** the user chooses Empty… on D: and then Cancel or Esc
- **THEN** nothing is erased
