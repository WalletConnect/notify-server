{
  app: {
    client_registrations:     (import 'app/client_registrations.libsonnet'      ).new,
    dispatched_notifications: (import 'app/dispatched_notifications.libsonnet'  ).new,
    send_failed:              (import 'app/send_failed.libsonnet'               ).new,
    account_not_found:        (import 'app/account_not_found.libsonnet'         ).new,
  },
  db: {
    available_memory:         (import 'db/available_memory.libsonnet'           ).new,
    cpu:                      (import 'db/cpu.libsonnet'                        ).new,
    net_throughput:           (import 'db/net_throughput.libsonnet'             ).new,
    write_latency:            (import 'db/write_latency.libsonnet'              ).new,
  },
  lb: {
    requests:                 (import 'lb/requests.libsonnet'                   ).new,
    error_4xx:                (import 'lb/error_4xx.libsonnet'                  ).new,
    error_5xx:                (import 'lb/error_5xx.libsonnet'                  ).new,
  },
}
