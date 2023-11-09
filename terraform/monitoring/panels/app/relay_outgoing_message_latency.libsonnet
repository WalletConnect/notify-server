local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Msg Out Latency',
      datasource  = ds.prometheus,
    )
    .configure(
      defaults.configuration.timeseries
        .withUnit('ms')
    )

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision) (rate(relay_outgoing_message_latency_sum[$__rate_interval])) / sum by (aws_ecs_task_revision) (rate(relay_outgoing_message_latency_count[$__rate_interval]))',
      legendFormat  = 'Publish w/ retries r{{aws_ecs_task_revision}}',
      exemplar      = false,
      refId         = 'RelayOutgoingMessageLatency',
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision) (rate(relay_outgoing_message_publish_latency_sum[$__rate_interval])) / sum by (aws_ecs_task_revision) (rate(relay_outgoing_message_publish_latency_count[$__rate_interval]))',
      legendFormat  = 'Individual RPC r{{aws_ecs_task_revision}}',
      exemplar      = false,
      refId         = 'RelayOutgoingMessagePublishLatency',
    ))
}
