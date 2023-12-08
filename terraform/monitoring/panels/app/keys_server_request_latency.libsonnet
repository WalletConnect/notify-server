local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Keys Server Req Latency',
      datasource  = ds.prometheus,
    )
    .configure(
      defaults.configuration.timeseries
        .withUnit('ms')
    )

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision) (rate(keys_server_request_latency_sum{source="server"}[$__rate_interval])) / sum by (aws_ecs_task_revision) (rate(keys_server_request_latency_count{source="server"}[$__rate_interval]))',
      legendFormat  = 'Server r{{aws_ecs_task_revision}}',
      exemplar      = false,
      refId         = 'KeysServerRequestLatencyServer',
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision) (rate(keys_server_request_latency_sum{source="cache"}[$__rate_interval])) / sum by (aws_ecs_task_revision) (rate(keys_server_request_latency_count{source="cache"}[$__rate_interval]))',
      legendFormat  = 'Cache r{{aws_ecs_task_revision}}',
      exemplar      = false,
      refId         = 'KeysServerRequestLatencyCache',
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision) (rate(keys_server_request_latency_sum[$__rate_interval])) / sum by (aws_ecs_task_revision) (rate(keys_server_request_latency_count[$__rate_interval]))',
      legendFormat  = 'Total r{{aws_ecs_task_revision}}',
      exemplar      = false,
      refId         = 'KeysServerRequestLatencyTotal',
    ))
}
