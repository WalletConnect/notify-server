local panels = (import '../grafonnet-lib/defaults.libsonnet').panels;
local docdb  = panels.aws.docdb;
local ecs    = panels.aws.ecs;
local units  = (import '../grafonnet-lib/utils/units.libsonnet');

# Make sure `docdb_mem_threshold` is 10% of total DocumentDB memory as per AWS guidance:
# "High RAM consumption — If your FreeableMemory metric frequently dips below 10% of the total instance memory, consider scaling up your instances."
# https://docs.aws.amazon.com/documentdb/latest/developerguide/best_practices.html#best_practices-performance_evaluating_metrics
# If you disagree with these instructions, please review this thread for context: https://walletconnect.slack.com/archives/C058RS0MH38/p1692548822748369?thread_ts=1692448085.704979&cid=C058RS0MH38
local docdb_mem = 16;
local docdb_mem_threshold = units.size_bin(GiB = docdb_mem * 0.1);

{
  app: {
    subscribed_topics:          (import 'app/subscribed_topics.libsonnet'         ).new,
    subscribe_latency:          (import 'app/subscribe_latency.libsonnet'         ).new,
    dispatched_notifications:   (import 'app/dispatched_notifications.libsonnet'  ).new,
    send_failed:                (import 'app/send_failed.libsonnet'               ).new,
    account_not_found:          (import 'app/account_not_found.libsonnet'         ).new,
    notify_latency:             (import 'app/notify_latency.libsonnet'            ).new,
    http_requests:              (import 'app/http_requests.libsonnet'             ).new,
    http_request_latency:       (import 'app/http_request_latency.libsonnet'      ).new,
  },
  db: {
    available_memory(ds, vars):         docdb.available_memory.panel(ds.cloudwatch, vars.namespace, vars.environment, vars.notifications, vars.docdb_cluster_id, mem_threshold = docdb_mem_threshold),
    buffer_cache_hit_ratio(ds, vars):   docdb.buffer_cache_hit_ratio.panel(ds.cloudwatch, vars.docdb_cluster_id),
    connections(ds, vars):              docdb.connections.panel(ds.cloudwatch, vars.docdb_cluster_id),
    cpu(ds, vars):                      docdb.cpu.panel(ds.cloudwatch, vars.namespace, vars.environment, vars.notifications, vars.docdb_cluster_id),
    low_mem_op_throttled(ds, vars):     docdb.low_mem_op_throttled.panel(ds.cloudwatch, vars.namespace, vars.environment, vars.notifications, vars.docdb_cluster_id),
    net_throughput(ds, vars):           docdb.net_throughput.panel(ds.cloudwatch, vars.docdb_cluster_id),
    volume(ds, vars):                   docdb.volume.panel(ds.cloudwatch, vars.docdb_cluster_id),
    write_latency(ds, vars):            docdb.write_latency.panel(ds.cloudwatch, vars.docdb_cluster_id),
  },
  ecs: {
    cpu(ds, vars):            ecs.cpu.panel(ds.cloudwatch, vars.namespace, vars.environment, vars.notifications, vars.ecs_service_name, vars.ecs_cluster_name),
    memory(ds, vars):         ecs.memory.panel(ds.cloudwatch, vars.namespace, vars.environment, vars.notifications, vars.ecs_service_name, vars.ecs_cluster_name),
  },
  lb: {
    active_connections:       (import 'lb/active_connections.libsonnet'         ).new,
    error_4xx:                (import 'lb/error_4xx.libsonnet'                  ).new,
    error_5xx:                (import 'lb/error_5xx.libsonnet'                  ).new,
    healthy_hosts:            (import 'lb/healthy_hosts.libsonnet'              ).new,
    requests:                 (import 'lb/requests.libsonnet'                   ).new,
  },
}
