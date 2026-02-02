---
type: project_command
description: create a bump version commit and a release-xxx pr for reviewing
---

Please create a new branch (named in the format release-xxx) on the current HEAD, then bump the software version to the version number specified by this command (modifications are required in both cargo.toml and .spec files. For the .spec file, you also need to update the changelog using the correct date of the day (you should get this via the date command). The changelog should be created based on the commit differences between the current and previous versions, excluding any commits that don't belong to this branch). After updating the version in cargo.toml, please update cargo.lock using the cargo command but ensure that only the current crate's version is updated without changing other dependency crate versions. Finally, please create a commit named "chore: bump version to xxxx". Then create a PR and submit it to GitHub for my review. That's it.
