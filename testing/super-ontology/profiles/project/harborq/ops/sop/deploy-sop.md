# Deploy SOP

Standard operating procedure for a release of Harbor Queue.

1. Tag the release with git tag -s v0.3.0 and push the tag.
2. Wait for the CI job named release to finish.
3. Roll the workers one at a time; never restart all workers at once.
4. Check the dead letter table is empty before closing the release.
