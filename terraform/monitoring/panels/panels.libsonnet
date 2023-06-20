{
  app: {
    client_registrations:     (import 'app/client_registrations.libsonnet'      ).new,
    dispatched_notifications: (import 'app/dispatched_notifications.libsonnet'  ).new,
    send_failed:              (import 'app/send_failed.libsonnet'               ).new,
    account_not_found:        (import 'app/account_not_found.libsonnet'         ).new,
  },
  db: {
    available_memory:         (import 'db/available_memory.libsonnet'           ).new,
    buffer_cache_hit_ratio:   (import 'db/buffer_cache_hit_ratio.libsonnet'     ).new,
    connections:              (import 'db/connections.libsonnet'                ).new,
    cpu:                      (import 'db/cpu.libsonnet'                        ).new,
    low_mem_op_throttled:     (import 'db/low_mem_op_throttled.libsonnet'       ).new,
    net_throughput:           (import 'db/net_throughput.libsonnet'             ).new,
    volume:                   (import 'db/volume.libsonnet'                     ).new,
    write_latency:            (import 'db/write_latency.libsonnet'              ).new,
  },
  ecs: {
    cpu:                      (import 'ecs/cpu.libsonnet'                       ).new,
    memory:                   (import 'ecs/memory.libsonnet'                    ).new,
  },
  lb: {
    active_connections:       (import 'lb/active_connections.libsonnet'         ).new,
    error_4xx:                (import 'lb/error_4xx.libsonnet'                  ).new,
    error_5xx:                (import 'lb/error_5xx.libsonnet'                  ).new,
    healthy_hosts:            (import 'lb/healthy_hosts.libsonnet'              ).new,
    requests:                 (import 'lb/requests.libsonnet'                   ).new,
  },
}
