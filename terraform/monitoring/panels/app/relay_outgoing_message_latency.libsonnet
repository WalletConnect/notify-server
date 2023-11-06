local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

local _configuration = defaults.configuration.timeseries
  .withUnit('ms')
  .withSoftLimit(
    axisSoftMin = 0,
    axisSoftMax = 2000,
  );

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Relay Outgoing Message Latency',
      datasource  = ds.prometheus,
    )
    .configure(_configuration)

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum(rate(relay_outgoing_message_latency[$__rate_interval])) / sum(rate(relay_outgoing_message_latency_count[$__rate_interval]))',
      legendFormat  = 'r{{aws_ecs_task_revision}}',
      exemplar      = false,
      refId         = 'RelayOutgoingMessageLatency',
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum(rate(relay_outgoing_message_publish_latency[$__rate_interval])) / sum(rate(relay_outgoing_message_publish_latency_count[$__rate_interval]))',
      legendFormat  = 'r{{aws_ecs_task_revision}}',
      exemplar      = false,
      refId         = 'RelayOutgoingMessagePublishLatency',
    ))
}
