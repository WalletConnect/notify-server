CREATE TABLE project (
    id          uuid        primary key default gen_random_uuid(),
    inserted_at timestamptz not null    default now(),
    updated_at  timestamptz not null    default now(),

    project_id varchar(255) not null unique,
    app_domain varchar(255) not null unique,
    topic      varchar(255) not null unique,

    authentication_public_key  varchar(255) not null unique,
    authentication_private_key varchar(255) not null unique,
    subscribe_public_key       varchar(255) not null unique,
    subscribe_private_key      varchar(255) not null unique
);
CREATE INDEX projects_project_id_idx ON project (project_id);
CREATE INDEX projects_app_domain_idx ON project (app_domain);
CREATE INDEX projects_topic_idx      ON project (topic);

CREATE TABLE subscriber (
    id          uuid        primary key default gen_random_uuid(),
    inserted_at timestamptz not null    default now(),
    updated_at  timestamptz not null    default now(),

    project uuid         not null references project (id),
    account varchar(255) not null,
    sym_key varchar(255) not null unique,
    topic   varchar(255) not null unique,
    expiry  timestamptz  not null,

    unique (project, account)
);
CREATE INDEX subscribers_project_idx ON subscriber (project);
CREATE INDEX subscribers_account_idx ON subscriber (account);
CREATE INDEX subscribers_topic_idx   ON subscriber (topic);
CREATE INDEX subscribers_expiry_idx  ON subscriber (expiry);

-- CREATE TABLE subscriber_scope (
--     id          uuid        primary key default gen_random_uuid(),
--     inserted_at timestamptz not null    default now(),
--     updated_at  timestamptz not null    default now(),

--     subscriber varchar(255) not null references subscriber (id),
--     name       varchar(255) not null,
--     unique (subscriber, name)
-- );

CREATE TABLE subscription_watcher (
    id          uuid        primary key default gen_random_uuid(),
    inserted_at timestamptz not null    default now(),
    updated_at  timestamptz not null    default now(),

    account varchar(255) not null,
    project uuid         references project (id),
    did_key varchar(255) not null unique,
    sym_key varchar(255) not null,
    expiry  timestamptz  not null
);
CREATE INDEX subscription_watcher_account_idx ON subscription_watcher (account);
CREATE INDEX subscription_watcher_project_idx ON subscription_watcher (project);
CREATE INDEX subscription_watcher_did_key_idx   ON subscription_watcher (did_key);
CREATE INDEX subscription_watcher_expiry_idx  ON subscription_watcher (expiry);
