# The notes example's strings, in English. Every string is here, so this is the
# language shown for whatever another language lacks.
#
# Each screen's strings start with its name. The app's own (the window, its toolbar
# and its menu) start with app-.

## The app

app-title = Notes
app-new-note = New Note
app-delete = Delete
app-menu-file = File
app-menu-delete-note = Delete Note
app-menu-folder = Folder
app-menu-new-folder = New Folder…
app-menu-rename-folder = Rename Folder…
app-menu-delete-folder = Delete Folder…
# $reason is the system's, in the system's language.
app-problem-read = Could not read the notes: { $reason }
app-problem-save = Could not save: { $reason }
app-problem-name-taken = Could not save: { $name } already exists
app-skipped-file = Could not read { $file }
app-skipped-files =
    { $count ->
        [one] Could not read { $count } file
       *[other] Could not read { $count } files
    }

## The sidebar

sidebar-all-notes = All Notes
sidebar-folders = Folders
sidebar-new-folder = New Folder
sidebar-rename = Rename…
sidebar-delete = Delete…
sidebar-name = Name
# Also the name of a folder made without typing one.
sidebar-default-name = New Folder
sidebar-create = Create
sidebar-rename-title = Rename Folder
sidebar-rename-button = Rename
sidebar-cancel = Cancel
sidebar-delete-title = Delete “{ $folder }”?
sidebar-delete-message =
    { $count ->
        [0] The folder is empty.
        [one] Its note goes to the trash (.trash in the notes folder) with it.
       *[other] Its { $count } notes go to the trash (.trash in the notes folder) with it.
    }
sidebar-delete-button = Delete

## The list

list-search = Search
# A note whose first line is empty.
list-untitled = New Note
list-just-now = Just now
list-yesterday = Yesterday
list-minutes-ago =
    { $count ->
        [one] { $count } minute ago
       *[other] { $count } minutes ago
    }
list-hours-ago =
    { $count ->
        [one] { $count } hour ago
       *[other] { $count } hours ago
    }
list-days-ago =
    { $count ->
        [one] { $count } day ago
       *[other] { $count } days ago
    }
list-weeks-ago =
    { $count ->
        [one] { $count } week ago
       *[other] { $count } weeks ago
    }
list-months-ago =
    { $count ->
        [one] { $count } month ago
       *[other] { $count } months ago
    }
list-years-ago =
    { $count ->
        [one] { $count } year ago
       *[other] { $count } years ago
    }

## The editor

editor-folder = Folder
editor-no-folder = No Folder
editor-note = Note
editor-no-notes = No notes here yet. Make one with New Note.
editor-none-selected = No note selected
