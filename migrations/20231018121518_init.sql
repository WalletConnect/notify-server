CREATE TABLE project (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    inserted_at TIMESTAMPTZ NOT NULL    DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL    DEFAULT now(),

    project_id VARCHAR(255) NOT NULL UNIQUE,
    app_domain VARCHAR(255) NOT NULL UNIQUE,
    topic      VARCHAR(255) NOT NULL UNIQUE,

    authentication_public_key  VARCHAR(255) NOT NULL UNIQUE,
    authentication_private_key VARCHAR(255) NOT NULL UNIQUE,
    subscribe_public_key       VARCHAR(255) NOT NULL UNIQUE,
    subscribe_private_key      VARCHAR(255) NOT NULL UNIQUE
);
CREATE INDEX projects_project_id_idx ON project (project_id);
CREATE INDEX projects_app_domain_idx ON project (app_domain);
CREATE INDEX projects_topic_idx      ON project (topic);

CREATE TABLE subscriber (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    inserted_at TIMESTAMPTZ NOT NULL    DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL    DEFAULT now(),

    project UUID         NOT NULL REFERENCES project (id) ON DELETE CASCADE,
    account VARCHAR(255) NOT NULL,
    sym_key VARCHAR(255) NOT NULL UNIQUE,
    topic   VARCHAR(255) NOT NULL UNIQUE,
    expiry  TIMESTAMPTZ  NOT NULL,

    UNIQUE (project, account)
);
CREATE INDEX subscribers_project_idx ON subscriber (project);
CREATE INDEX subscribers_account_idx ON subscriber (account);
CREATE INDEX subscribers_topic_idx   ON subscriber (topic);
CREATE INDEX subscribers_expiry_idx  ON subscriber (expiry);

CREATE TABLE subscriber_scope (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    inserted_at TIMESTAMPTZ NOT NULL    DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL    DEFAULT now(),

    subscriber UUID         NOT NULL REFERENCES subscriber (id) ON DELETE CASCADE,
    name       VARCHAR(255) NOT NULL,

    UNIQUE (subscriber, name)
);
CREATE INDEX subscriber_scope_subscriber_idx ON subscriber_scope (subscriber);
CREATE INDEX subscriber_scope_name_idx       ON subscriber_scope (name);

CREATE TABLE subscription_watcher (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    inserted_at TIMESTAMPTZ NOT NULL    DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL    DEFAULT now(),

    account VARCHAR(255) NOT NULL,
    project UUID         REFERENCES project (id) ON DELETE CASCADE,
    did_key VARCHAR(255) NOT NULL UNIQUE,
    sym_key VARCHAR(255) NOT NULL,
    expiry  TIMESTAMPTZ  NOT NULL
);
CREATE INDEX subscription_watcher_account_idx ON subscription_watcher (account);
CREATE INDEX subscription_watcher_project_idx ON subscription_watcher (project);
CREATE INDEX subscription_watcher_did_key_idx ON subscription_watcher (did_key);
CREATE INDEX subscription_watcher_expiry_idx  ON subscription_watcher (expiry);
