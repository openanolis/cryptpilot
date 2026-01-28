---
type: project_command
description: create a bump version commit and a release-xxx pr for reviewing
---

Please create a new branch (named in the format release-xxx) on the current HEAD, then bump the software version to the version number specified by this command (modifications are required in both cargo.toml and .spec files. For the .spec file, you also need to update the changelog using the correct date of the day (you should get this via the date command). The changelog should be created based on the commit differences between the current and previous versions, excluding any commits that don't belong to this branch). Then use the cargo command to update the version number in cargo.lock. Finally, please create a commit named "chore: bump version to xxxx". Then create a PR and submit it to GitHub for my review. That's it.
