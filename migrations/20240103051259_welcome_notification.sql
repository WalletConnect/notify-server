CREATE TABLE welcome_notification (
    id              uuid         PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at      timestamptz  NOT NULL DEFAULT now(),
    updated_at      timestamptz  NOT NULL DEFAULT now(),
    project         uuid         NOT NULL REFERENCES project (id) ON DELETE CASCADE,
    type            uuid         NOT NULL,
    title           varchar(255) NOT NULL,
    body            varchar(255) NOT NULL,
    url             varchar(255) NULL,

    UNIQUE (project)
);
CREATE INDEX welcome_notification_project_idx ON welcome_notification (project);
