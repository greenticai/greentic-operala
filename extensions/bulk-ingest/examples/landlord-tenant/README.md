# Landlord/Tenant Bulk Upload Prompt

This example describes a desired OperaLa bulk-upload capability over the
landlord/tenant SoRLa contract from the sibling `greentic-sorla` repository.

```bash
greentic-operala prompt \
  --locale en-GB \
  --tenant demo-landlord \
  --team property-ops \
  --sorla ../greentic-sorla/examples/landlord-tenant/sorla.yaml \
  --output target/operala-demo/landlord-tenant-bulk-upload.answers.json \
  "$(cat extensions/bulk-ingest/examples/landlord-tenant/prompt.txt)"
```

OperaLa authors this with the generic `greentic.operala.bulk_ingest.v1`
extension. The generated handoff describes the batch schema, record collections,
SoRLa action bindings, expected counts, and validation posture. `greentic-operax`
will need runner support for the `bulk_ingest` handoff before the generated pack
can execute the upload against SORX.
