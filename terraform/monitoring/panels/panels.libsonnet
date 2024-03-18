local panels = (import '../grafonnet-lib/defaults.libsonnet').panels;
local docdb  = panels.aws.docdb;
local ecs    = panels.aws.ecs;
local redis  = panels.aws.redis;
local units  = (import '../grafonnet-lib/utils/units.libsonnet');

# Make sure `docdb_mem_threshold` is 10% of total DocumentDB memory as per AWS guidance:
# "High RAM consumption — If your FreeableMemory metric frequently dips below 10% of the total instance memory, consider scaling up your instances."
# https://docs.aws.amazon.com/documentdb/latest/developerguide/best_practices.html#best_practices-performance_evaluating_metrics
# If you disagree with these instructions, please review this thread for context: https://walletconnect.slack.com/archives/C058RS0MH38/p1692548822748369?thread_ts=1692448085.704979&cid=C058RS0MH38
local docdb_mem = 16;
local docdb_mem_threshold = units.size_bin(GiB = docdb_mem * 0.1);

{
  app: {
    http_request_rate:                          (import 'app/http_request_rate.libsonnet'                          ).new,
    http_request_latency:                       (import 'app/http_request_latency.libsonnet'                       ).new,
    subscribed_topics:                          (import 'app/subscribed_topics.libsonnet'                          ).new,
    subscribe_latency:                          (import 'app/subscribe_latency.libsonnet'                          ).new,
    relay_incoming_message_rate:                (import 'app/relay_incoming_message_rate.libsonnet'                ).new,
    relay_incoming_message_latency:             (import 'app/relay_incoming_message_latency.libsonnet'             ).new,
    relay_incoming_message_server_errors:       (import 'app/relay_incoming_message_server_errors.libsonnet'       ).new,
    relay_outgoing_message_rate:                (import 'app/relay_outgoing_message_rate.libsonnet'                ).new,
    relay_outgoing_message_latency:             (import 'app/relay_outgoing_message_latency.libsonnet'             ).new,
    relay_outgoing_message_failures:            (import 'app/relay_outgoing_message_failures.libsonnet'            ).new,
    relay_subscribe_rate:                       (import 'app/relay_subscribe_rate.libsonnet'                       ).new,
    relay_subscribe_latency:                    (import 'app/relay_subscribe_latency.libsonnet'                    ).new,
    relay_subscribe_failures:                   (import 'app/relay_subscribe_failures.libsonnet'                   ).new,
    postgres_query_rate:                        (import 'app/postgres_query_rate.libsonnet'                        ).new,
    postgres_query_latency:                     (import 'app/postgres_query_latency.libsonnet'                     ).new,
    keys_server_request_rate:                   (import 'app/keys_server_request_rate.libsonnet'                   ).new,
    keys_server_request_latency:                (import 'app/keys_server_request_latency.libsonnet'                ).new,
    registry_request_rate:                      (import 'app/registry_request_rate.libsonnet'                      ).new,
    registry_request_latency:                   (import 'app/registry_request_latency.libsonnet'                   ).new,
    notify_latency:                             (import 'app/notify_latency.libsonnet'                             ).new,
    dispatched_notifications:                   (import 'app/dispatched_notifications.libsonnet'                   ).new,
    send_failed:                                (import 'app/send_failed.libsonnet'                                ).new,
    account_not_found:                          (import 'app/account_not_found.libsonnet'                          ).new,
    publishing_workers_count:                   (import 'app/publishing_workers_count.libsonnet'                   ).new,
    publishing_workers_errors:                  (import 'app/publishing_workers_errors.libsonnet'                  ).new,
    publishing_workers_queued_size:             (import 'app/publishing_workers_queued_size.libsonnet'             ).new,
    publishing_workers_processing_size:         (import 'app/publishing_workers_processing_size.libsonnet'         ).new,
    publishing_workers_published_count:         (import 'app/publishing_workers_published_count.libsonnet'         ).new,
  },
  ecs: {
    cpu(ds, vars):            ecs.cpu.panel(ds.cloudwatch, vars.namespace, vars.environment, vars.notifications, vars.ecs_service_name, vars.ecs_cluster_name),
    memory(ds, vars):         ecs.memory.panel(ds.cloudwatch, vars.namespace, vars.environment, vars.notifications, vars.ecs_service_name, vars.ecs_cluster_name),
  },
  rds: {
    cpu:                   (import 'rds/cpu.libsonnet'                   ).new,
    freeable_memory:       (import 'rds/freeable_memory.libsonnet'       ).new,
    volume_bytes_used:     (import 'rds/volume_bytes_used.libsonnet'     ).new,
    database_connections:  (import 'rds/database_connections.libsonnet'  ).new,
  },
  redis: {
    cpu(ds, vars):            redis.cpu.panel(ds.cloudwatch, vars.namespace, vars.environment, vars.notifications, vars.redis_cluster_id),
    memory(ds, vars):         redis.memory.panel(ds.cloudwatch, vars.namespace, vars.environment, vars.notifications, vars.redis_cluster_id),
  },
  lb: {
    active_connections:       (import 'lb/active_connections.libsonnet'         ).new,
    error_4xx:                (import 'lb/error_4xx.libsonnet'                  ).new,
    error_5xx:                (import 'lb/error_5xx.libsonnet'                  ).new,
    error_5xx_logs:           (import 'lb/error_5xx_logs.libsonnet'             ).new,
    healthy_hosts:            (import 'lb/healthy_hosts.libsonnet'              ).new,
    requests:                 (import 'lb/requests.libsonnet'                   ).new,
  },
}
