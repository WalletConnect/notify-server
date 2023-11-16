local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Msg In Server Errors',
      datasource  = ds.prometheus,
    )
    .configure(defaults.configuration.timeseries)

    .setAlert(vars.environment, grafana.alert.new(
      namespace     = vars.namespace,
      name          = '%(env)s - Failed to process incoming relay message' % { env: vars.environment },
      message       = '%(env)s - Failed to process incoming relay message' % { env: vars.environment },
      notifications = vars.notifications,
      noDataState   = 'no_data',
      conditions    = [
        grafana.alertCondition.new(
          evaluatorParams = [ 1 ],
          evaluatorType   = 'gt',
          operatorType    = 'or',
          queryRefId      = 'RelayIncomingMessagesServerErrors',
          queryTimeStart  = '5m',
          queryTimeEnd    = 'now',
          reducerType     = grafana.alert_reducers.Avg
        ),
      ],
    ))

    .addTarget(targets.prometheus(
      datasource    = ds.prometheus,
      expr          = 'sum by (aws_ecs_task_revision, tag) (increase(relay_incoming_messages_total{status="server_error"}[$__rate_interval]))',
      legendFormat  = '{{tag}} r{{aws_ecs_task_revision}}',
      exemplar      = true,
      refId         = 'RelayIncomingMessagesServerErrors',
    ))
}
