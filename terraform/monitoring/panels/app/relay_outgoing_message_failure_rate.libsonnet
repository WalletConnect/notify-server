local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Relay Outgoing Messages Failures Rate',
      datasource  = ds.prometheus,
    )
    .configure(defaults.configuration.timeseries)

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum(rate(relay_outgoing_message_failures[$__rate_interval]))',
      legendFormat  = '{{is_permenant}} r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'RelayOutgoingMessageFailuresRate',
    ))
}
