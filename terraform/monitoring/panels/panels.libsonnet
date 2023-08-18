local panels = (import '../grafonnet-lib/defaults.libsonnet').panels;
local docdb  = panels.aws.docdb;
local ecs    = panels.aws.ecs;

{
  app: {
    subscribed_project_topics:  (import 'app/subscribed_project_topics.libsonnet'      ).new,
    subscribed_client_topics:   (import 'app/subscribed_client_topics.libsonnet'      ).new,
    dispatched_notifications:   (import 'app/dispatched_notifications.libsonnet'  ).new,
    send_failed:                (import 'app/send_failed.libsonnet'               ).new,
    account_not_found:          (import 'app/account_not_found.libsonnet'         ).new,
  },
  db: {
    available_memory(ds, vars):         docdb.available_memory.panel(ds.cloudwatch, vars.namespace, vars.environment, vars.notifications, vars.docdb_cluster_id),
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
