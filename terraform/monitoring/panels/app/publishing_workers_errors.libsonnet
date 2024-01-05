local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'Publishing worker tasks errors count',
      datasource  = ds.prometheus,
    )
    .configure(defaults.configuration.timeseries)

    .setAlert(vars.environment, grafana.alert.new(
      namespace     = vars.namespace,
      name          = '%(env)s - Notify publisher worker task error' % { env: vars.environment },
      message       = '%(env)s - Notify publisher worker task error' % { env: vars.environment },
      notifications = vars.notifications,
      noDataState   = 'no_data',
      period        = '0m',
      conditions    = [
        grafana.alertCondition.new(
          evaluatorParams = [ 0 ],
          evaluatorType   = 'gt',
          operatorType    = 'or',
          queryRefId      = 'PublishingWorkersErrors',
          queryTimeStart  = '1m',
          queryTimeEnd    = 'now',
          reducerType     = grafana.alert_reducers.Avg
        ),
      ],
    ))

    .addTarget(targets.prometheus(
      datasource  = ds.prometheus,
      expr        = 'sum(rate(publishing_workers_errors_total{}[$__rate_interval])) or vector(0)',
      refId       = 'PublishingWorkersErrors',
    ))
}
