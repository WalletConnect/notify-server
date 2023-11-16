local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Msg In Rate',
      datasource  = ds.prometheus,
    )
    .configure(
      defaults.configuration.timeseries
        .withUnit('cps')
    )

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision, tag) (rate(relay_incoming_messages_total[$__rate_interval]))',
      legendFormat  = '{{tag}} r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'RelayIncomingMessagesRate',
    ))
}
