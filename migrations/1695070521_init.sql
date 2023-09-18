CREATE TABLE projects (
    id          varchar(255) primary key default gen_random_uuid(),
    inserted_at timestamptz  not null    default now(),

    project_id varchar(255) not null unique,
    app_domain varchar(255) not null unique,
    topic      varchar(255) not null unique,

    authentication_public_key  varchar(255) not null unique,
    authentication_private_key varchar(255) not null unique,
    subscribe_public_key       varchar(255) not null unique,
    subscribe_private_key      varchar(255) not null unique
);
CREATE INDEX projects_project_id_idx ON projects (project_id);
CREATE INDEX projects_app_domain_idx ON projects (app_domain);
CREATE INDEX projects_topic_idx ON projects (topic);
