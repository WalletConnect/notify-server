CREATE TYPE notification_states
  AS ENUM ('queued', 'processing', 'published', 'not-subscribed', 'wrong-scope', 'rate-limited');

CREATE TABLE notification (
    id         UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    project    UUID         NOT NULL REFERENCES project (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ  NOT NULL DEFAULT now(),
    type       VARCHAR(255) NOT NULL,
    title      VARCHAR(255) NOT NULL,
    body       VARCHAR(255) NOT NULL,
    icon       VARCHAR(255) NOT NULL,
    url        VARCHAR(255) NOT NULL
);
CREATE INDEX notification_project_id ON notification (project);

CREATE TABLE subscriber_notification (
    id            UUID                 PRIMARY KEY DEFAULT gen_random_uuid(),
    notification  UUID                 NOT NULL REFERENCES notification (id) ON DELETE CASCADE,
    subscriber    UUID                 NOT NULL REFERENCES subscriber (id) ON DELETE CASCADE,
    created_at    TIMESTAMPTZ          NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ          NOT NULL DEFAULT now(),
    status        notification_states  NOT NULL
);
CREATE INDEX subscriber_notification_subscriber_id ON subscriber_notification (subscriber);
CREATE INDEX subscriber_notification_notification_id ON subscriber_notification (notification);
CREATE INDEX subscriber_notification_status ON subscriber_notification (status);
CREATE INDEX subscriber_notification_created_and_updated ON subscriber_notification (created_at, updated_at);
