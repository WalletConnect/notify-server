CREATE TABLE project (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    inserted_at timestamptz NOT NULL    DEFAULT now(),
    updated_at  timestamptz NOT NULL    DEFAULT now(),

    project_id varchar(255) NOT NULL UNIQUE,
    app_domain varchar(255) NOT NULL UNIQUE,
    topic      varchar(255) NOT NULL UNIQUE,

    authentication_public_key  varchar(255) NOT NULL UNIQUE,
    authentication_private_key varchar(255) NOT NULL UNIQUE,
    subscribe_public_key       varchar(255) NOT NULL UNIQUE,
    subscribe_private_key      varchar(255) NOT NULL UNIQUE
);
CREATE INDEX projects_project_id_idx ON project (project_id);
CREATE INDEX projects_app_domain_idx ON project (app_domain);
CREATE INDEX projects_topic_idx      ON project (topic);

CREATE TABLE subscriber (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    inserted_at timestamptz NOT NULL    DEFAULT now(),
    updated_at  timestamptz NOT NULL    DEFAULT now(),

    project uuid         NOT NULL REFERENCES project (id) ON DELETE CASCADE,
    account varchar(255) NOT NULL,
    sym_key varchar(255) NOT NULL UNIQUE,
    topic   varchar(255) NOT NULL UNIQUE,
    expiry  timestamptz  NOT NULL,

    UNIQUE (project, account)
);
CREATE INDEX subscribers_project_idx ON subscriber (project);
CREATE INDEX subscribers_account_idx ON subscriber (account);
CREATE INDEX subscribers_topic_idx   ON subscriber (topic);
CREATE INDEX subscribers_expiry_idx  ON subscriber (expiry);

CREATE TABLE subscriber_scope (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    inserted_at timestamptz NOT NULL    DEFAULT now(),
    updated_at  timestamptz NOT NULL    DEFAULT now(),

    subscriber uuid         NOT NULL REFERENCES subscriber (id) ON DELETE CASCADE,
    name       varchar(255) NOT NULL,

    UNIQUE (subscriber, name)
);
CREATE INDEX subscriber_scope_subscriber_idx ON subscriber_scope (subscriber);
CREATE INDEX subscriber_scope_name_idx       ON subscriber_scope (name);

CREATE TABLE subscription_watcher (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    inserted_at timestamptz NOT NULL    DEFAULT now(),
    updated_at  timestamptz NOT NULL    DEFAULT now(),

    account varchar(255) NOT NULL,
    project uuid         REFERENCES project (id) ON DELETE CASCADE,
    did_key varchar(255) NOT NULL UNIQUE,
    sym_key varchar(255) NOT NULL,
    expiry  timestamptz  NOT NULL
);
CREATE INDEX subscription_watcher_account_idx ON subscription_watcher (account);
CREATE INDEX subscription_watcher_project_idx ON subscription_watcher (project);
CREATE INDEX subscription_watcher_did_key_idx ON subscription_watcher (did_key);
CREATE INDEX subscription_watcher_expiry_idx  ON subscription_watcher (expiry);
