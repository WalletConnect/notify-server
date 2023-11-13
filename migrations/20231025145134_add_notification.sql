CREATE TABLE notification (
    id              uuid         PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at      timestamptz  NOT NULL DEFAULT now(),
    updated_at      timestamptz  NOT NULL DEFAULT now(),
    project         uuid         NOT NULL REFERENCES project (id) ON DELETE CASCADE,
    notification_id varchar(255) NOT NULL,
    type            uuid         NOT NULL,
    title           varchar(255) NOT NULL,
    body            varchar(255) NOT NULL,
    icon            varchar(255) NULL,
    url             varchar(255) NULL,

    UNIQUE (project, notification_id)
);
CREATE INDEX notification_project_idx ON notification (project);
CREATE INDEX notification_notification_id_idx ON notification (notification_id);
CREATE INDEX notification_type_idx ON notification (type);

CREATE TYPE subscriber_notification_status
  AS ENUM ('queued', 'processing', 'published', 'failed');

CREATE TABLE subscriber_notification (
    id            uuid                            PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at    timestamptz                     NOT NULL DEFAULT now(),
    updated_at    timestamptz                     NOT NULL DEFAULT now(),
    notification  uuid                            NOT NULL REFERENCES notification (id) ON DELETE CASCADE,
    subscriber    uuid                            NOT NULL REFERENCES subscriber (id) ON DELETE CASCADE,
    status        subscriber_notification_status  NOT NULL,

    UNIQUE (notification, subscriber)
);
CREATE INDEX subscriber_notification_notification_idx ON subscriber_notification (notification);
CREATE INDEX subscriber_notification_subscriber_idx ON subscriber_notification (subscriber);
CREATE INDEX subscriber_notification_status_idx ON subscriber_notification (status);
