-- SPDX-License-Identifier: Apache-2.0
-- Synthetic predicate fixture for an isolated ephemeral PostgreSQL database.
-- It exercises the exact query, not migrations, worker execution or authorization.
CREATE TABLE public.import_jobs(tenant_id uuid,user_id uuid,id uuid,artwork_id uuid,original_asset_id uuid,asset_id uuid,
 state text,progress int,error_code text,png_drain_version int,lease_id uuid,lease_until timestamptz,png_worker_tokens text[],
 mime text,derived jsonb,derived_object_key text,model_derivation jsonb,idempotency_key text,expected_hash text,
 expected_bytes bigint,scale_meters double precision,model_preparation jsonb,rights jsonb,object_key text,approved_key text);
CREATE TABLE public.artworks(tenant_id uuid,id uuid,revision int,approved_revision int,approved_asset_id uuid,deleted_at timestamptz,metadata jsonb);
CREATE TABLE public.artwork_revisions(tenant_id uuid,artwork_id uuid,revision int,snapshot jsonb);
CREATE TABLE public.assets(tenant_id uuid,id uuid,artwork_id uuid,rights_id uuid,mime text,sha256 text,bytes bigint,
 scale_meters double precision,state text,deleted_at timestamptz,object_key text);
CREATE TABLE public.rights(tenant_id uuid,id uuid,metadata jsonb,deleted_at timestamptz);
CREATE TABLE public.asset_derivatives(tenant_id uuid,id uuid,asset_id uuid,metadata jsonb,deleted_at timestamptz);
CREATE TABLE public.artwork_approvals(tenant_id uuid,artwork_id uuid);
CREATE TABLE public.exhibitions(tenant_id uuid,id uuid,metadata jsonb,deleted_at timestamptz);
INSERT INTO public.rights VALUES('10000000-0000-4000-8000-000000000002','10000000-0000-4000-8000-000000000009','{"permissions":{"display":true}}',NULL);
INSERT INTO public.artworks VALUES('10000000-0000-4000-8000-000000000002','10000000-0000-4000-8000-000000000005',1,NULL,NULL,NULL,'{"rights":{"permissions":{"display":true}}}');
INSERT INTO public.artwork_revisions SELECT tenant_id,id,revision,metadata FROM public.artworks;
INSERT INTO public.assets VALUES
('10000000-0000-4000-8000-000000000002','10000000-0000-4000-8000-000000000007','10000000-0000-4000-8000-000000000005','10000000-0000-4000-8000-000000000009','model/gltf-binary',repeat('a',64),100,1,'quarantine',NULL,'10000000-0000-4000-8000-000000000002/quarantine/import-10000000-0000-4000-8000-000000000004/'||repeat('a',64)),
('10000000-0000-4000-8000-000000000002','10000000-0000-4000-8000-000000000008','10000000-0000-4000-8000-000000000005','10000000-0000-4000-8000-000000000009','model/gltf-binary',repeat('b',64),80,1,'approved',NULL,'10000000-0000-4000-8000-000000000002/approved/'||repeat('b',64));
INSERT INTO public.import_jobs VALUES(
'10000000-0000-4000-8000-000000000002','10000000-0000-4000-8000-000000000003','10000000-0000-4000-8000-000000000004','10000000-0000-4000-8000-000000000005','10000000-0000-4000-8000-000000000007','10000000-0000-4000-8000-000000000008',
'approved',100,NULL,1,NULL,NULL,'{}','model/gltf-binary',NULL,NULL,NULL,'10000000-0000-4000-8000-000000000006',repeat('a',64),100,1,
jsonb_build_object('kind','required-model-compact','request',jsonb_build_object('profileId','embedded-model-compact-20k-512-v1','operationID','10000000-0000-4000-8000-000000000006','generation','synthetic-required-model','artworkRevision',1),
'result',jsonb_build_object('sha256',repeat('c',64),'jobId','10000000-0000-4000-8000-000000000004','artworkId','10000000-0000-4000-8000-000000000005','artworkRevision',1,'operationID','10000000-0000-4000-8000-000000000006','generation','synthetic-required-model','profileId','embedded-model-compact-20k-512-v1','rightsSHA256',repeat('e',64),
'original',jsonb_build_object('assetId','10000000-0000-4000-8000-000000000007','sha256',repeat('a',64),'bytes',100),
'display',jsonb_build_object('assetId','10000000-0000-4000-8000-000000000008','sha256',repeat('b',64),'bytes',80))),
'{"permissions":{"display":true}}','10000000-0000-4000-8000-000000000002/quarantine/import-10000000-0000-4000-8000-000000000004/'||repeat('a',64),'10000000-0000-4000-8000-000000000002/approved/'||repeat('b',64));
INSERT INTO public.asset_derivatives SELECT tenant_id,'10000000-0000-4000-8000-000000000010',original_asset_id,
jsonb_build_object('version',1,'kind','required-model-preparation','originalAssetId',original_asset_id::text,'displayAssetId',asset_id::text,'serverReceipt',model_preparation->'result','provenance','server-generated'),NULL FROM public.import_jobs;
INSERT INTO public.exhibitions VALUES('10000000-0000-4000-8000-000000000002','10000000-0000-4000-8000-000000000011','{"candidate":{"artworks":[]},"editVersion":99}',NULL);
