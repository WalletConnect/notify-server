local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Relay Outgoing Messages Rate',
      datasource  = ds.prometheus,
    )
    .configure(defaults.configuration.timeseries)

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum(rate(relay_outgoing_messages[$__rate_interval]))',
      legendFormat  = '{{tag}} r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'RelayOutgoingMessagesRate',
    ))
}
